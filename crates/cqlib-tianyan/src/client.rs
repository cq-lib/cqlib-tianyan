// This code is part of Cqlib.
//
// (C) Copyright China Telecom Quantum Group 2026
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
//
// Any modifications or derivative works of this code must retain this
// copyright notice, and modified files need to carry a notice indicating
// that they have been altered from the originals.
// Modified to support filtered device queries and endpoint-specific response codes.

//! Authenticated HTTP client for the Tianyan quantum cloud platform.
//!
//! [`TianyanClient`] wraps [`reqwest::blocking::Client`] and handles:
//! - Injecting `basicToken` / `Authorization` headers on every request.
//! - Automatically re-logging in and retrying once on a `401 Unauthorized` response.
//! - Exponential-backoff retry for 5xx / network errors (max 2 retries).
//! - 429 Rate-Limit handling with `Retry-After` header support.
//! - Deserialising the platform's standard `{code, data, message}` envelope.

use crate::auth::{self, Credentials};
use crate::config::TianyanConfig;
use crate::error::TianyanError;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Maximum number of retries for transient (5xx / network) errors.
const MAX_RETRIES: u32 = 2;
/// Base delay for exponential backoff.
const BASE_RETRY_DELAY: Duration = Duration::from_secs(1);

/// Standard response envelope used by all Tianyan platform APIs.
#[derive(Deserialize)]
pub struct ApiResponse<T> {
    pub code: i64,
    /// Some endpoints use `message`, others use `msg` — accept both.
    #[serde(alias = "msg", default)]
    pub message: Option<String>,
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    /// Unwrap the `data` field, converting a non-zero `code` into a [`TianyanError::Api`].
    pub fn into_data(self) -> Result<T, TianyanError> {
        self.into_data_with_success_code(0)
    }

    /// Unwrap `data` for an endpoint with a different success code.
    pub fn into_data_with_success_code(self, success_code: i64) -> Result<T, TianyanError> {
        if self.code != success_code {
            return Err(TianyanError::Api {
                code: self.code,
                message: self
                    .message
                    .unwrap_or_else(|| format!("API error code {}", self.code)),
            });
        }
        self.data.ok_or_else(|| TianyanError::Api {
            code: self.code,
            message: "response 'data' field is null".to_string(),
        })
    }
}

/// Synchronous HTTP client with automatic token management and retry logic.
pub struct TianyanClient {
    http: reqwest::blocking::Client,
    pub(crate) config: TianyanConfig,
    credentials: Arc<Mutex<Credentials>>,
}

impl TianyanClient {
    /// Construct a new client from existing credentials.
    ///
    /// Returns an error if the underlying HTTP client cannot be initialised
    /// (extremely rare — only if OS TLS setup fails).
    pub fn new(config: TianyanConfig, credentials: Credentials) -> Result<Self, TianyanError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(TianyanError::Http)?;
        Ok(Self {
            http,
            config,
            credentials: Arc::new(Mutex::new(credentials)),
        })
    }

    /// Return a clone of the current credentials.
    pub fn credentials(&self) -> Result<Credentials, TianyanError> {
        self.credentials
            .lock()
            .map(|g| g.clone())
            .map_err(|_| TianyanError::Auth("credential mutex poisoned".into()))
    }

    /// Build the two auth headers expected by the Tianyan platform.
    fn auth_headers(token: &str) -> reqwest::header::HeaderMap {
        use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
        let mut headers = HeaderMap::new();
        if let (Ok(k1), Ok(v1)) = (
            HeaderName::from_bytes(b"basicToken"),
            HeaderValue::from_str(token),
        ) {
            headers.insert(k1, v1);
        }
        if let Ok(bearer) = HeaderValue::from_str(&format!("Bearer {}", token)) {
            headers.insert(reqwest::header::AUTHORIZATION, bearer);
        }
        headers
    }

    /// Re-login using the stored `api_key` and update the token in place.
    fn refresh_token(&self) -> Result<String, TianyanError> {
        let api_key = {
            let guard = self
                .credentials
                .lock()
                .map_err(|_| TianyanError::Auth("credential mutex poisoned".into()))?;
            guard.api_key.clone()
        };
        let new_creds = auth::login(&api_key, &self.config)?;
        let token = new_creds.access_token.clone();

        // Persist refreshed credentials
        auth::save_credentials(&new_creds, &self.config.credentials_path)?;

        {
            let mut guard = self
                .credentials
                .lock()
                .map_err(|_| TianyanError::Auth("credential mutex poisoned".into()))?;
            *guard = new_creds;
        }
        Ok(token)
    }

    /// Get the current token, proactively refreshing if expired.
    fn current_token(&self) -> Result<String, TianyanError> {
        let guard = self
            .credentials
            .lock()
            .map_err(|_| TianyanError::Auth("credential mutex poisoned".into()))?;
        if guard.is_token_expired() {
            drop(guard);
            self.refresh_token()
        } else {
            Ok(guard.access_token.clone())
        }
    }

    /// Build and send a single request (no retry logic).
    fn build_and_send(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::blocking::Response, reqwest::Error> {
        let mut req = match method {
            "POST" => self.http.post(url),
            _ => self.http.get(url),
        };
        req = req.headers(Self::auth_headers(token));
        if let Some(b) = body {
            req = req.json(b);
        }
        req.send()
    }

    /// Execute an HTTP request with retry logic:
    /// - 401 → re-login and retry once
    /// - 5xx / network error → exponential backoff, max MAX_RETRIES
    /// - 429 → read Retry-After header, wait, retry once
    fn send_with_retry<T: DeserializeOwned>(
        &self,
        method: &str,
        url: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, TianyanError> {
        let token = self.current_token()?;
        let mut last_err: Option<TianyanError> = None;

        for attempt in 0..=MAX_RETRIES {
            let result = self.build_and_send(method, url, &token, body);

            match result {
                Ok(resp) => {
                    let status = resp.status();

                    // 401 Unauthorized → re-login and retry once
                    if status == reqwest::StatusCode::UNAUTHORIZED {
                        let new_token = self.refresh_token()?;
                        return self
                            .build_and_send(method, url, &new_token, body)
                            .map_err(TianyanError::Http)?
                            .json::<T>()
                            .map_err(TianyanError::Http);
                    }

                    // 429 Too Many Requests → wait and retry
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let wait_secs = resp
                            .headers()
                            .get("Retry-After")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(5);
                        std::thread::sleep(Duration::from_secs(wait_secs));
                        last_err = Some(TianyanError::Api {
                            code: 429,
                            message: "rate limited".to_string(),
                        });
                        continue;
                    }

                    // 5xx → retry with backoff
                    if status.is_server_error() {
                        last_err = Some(TianyanError::Api {
                            code: status.as_u16() as i64,
                            message: format!("server error {}", status),
                        });
                        if attempt < MAX_RETRIES {
                            let delay = BASE_RETRY_DELAY * 2u32.pow(attempt);
                            std::thread::sleep(delay);
                        }
                        continue;
                    }

                    // Success: deserialise
                    return resp.json::<T>().map_err(TianyanError::Http);
                }
                Err(e) => {
                    last_err = Some(TianyanError::Http(e));
                    if attempt < MAX_RETRIES {
                        let delay = BASE_RETRY_DELAY * 2u32.pow(attempt);
                        std::thread::sleep(delay);
                    }
                }
            }
        }

        Err(last_err.unwrap())
    }

    /// Authenticated GET request; deserialises into `ApiResponse<T>`.
    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<ApiResponse<T>, TianyanError> {
        let url = format!("{}{}", self.config.base_url(), path);
        self.send_with_retry::<ApiResponse<T>>("GET", &url, None)
    }

    /// Authenticated GET request with URL-encoded query pairs. Repeated keys are preserved.
    pub fn get_with_query<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<ApiResponse<T>, TianyanError> {
        let url = format!("{}{}", self.config.base_url(), path);
        let mut url = reqwest::Url::parse(&url)
            .map_err(|e| TianyanError::InvalidInput(format!("invalid request URL: {e}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(
                query
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_str())),
            );
        }
        self.send_with_retry::<ApiResponse<T>>("GET", url.as_str(), None)
    }

    /// Authenticated POST request with a JSON body; deserialises into `ApiResponse<T>`.
    pub fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<ApiResponse<T>, TianyanError> {
        let url = format!("{}{}", self.config.base_url(), path);
        let json_body = serde_json::to_value(body).map_err(TianyanError::Json)?;
        self.send_with_retry::<ApiResponse<T>>("POST", &url, Some(&json_body))
    }
}
