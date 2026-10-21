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

//! Authentication and credential persistence for the Tianyan platform.
//!
//! Credentials are stored as JSON at the path defined in [`TianyanConfig::credentials_path`]
//! (defaults to `~/.cqlib/tianyan/credentials.json`).  The file contains both the original
//! `api_key` (openId) and the current `access_token` so the library can re-login
//! automatically when the token expires, without user interaction.
//!
//! # Token lifetime
//!
//! The Tianyan platform does not publish a formal expiry duration; in practice tokens
//! appear to remain valid for several hours.  This library conservatively treats a token as
//! expired after [`TOKEN_VALID_SECS`] seconds (3 hours) and re-logs in proactively before
//! making API calls.

use crate::config::{LOGIN_PATH, TianyanConfig};
use crate::error::TianyanError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Conservative token lifetime in seconds (3 hours).
pub const TOKEN_VALID_SECS: i64 = 3 * 60 * 60;

/// Persisted authentication credentials for the Tianyan platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// The user's permanent API key (openId), used to obtain new access tokens.
    pub api_key: String,
    /// The current Bearer access token returned by the login endpoint.
    pub access_token: String,
    /// Unix timestamp (seconds) when `access_token` was obtained.
    pub token_obtained_at: i64,
}

impl Credentials {
    /// Returns `true` if the cached access token has exceeded [`TOKEN_VALID_SECS`].
    pub fn is_token_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now - self.token_obtained_at >= TOKEN_VALID_SECS
    }
}

/// Login request body sent to the Tianyan auth endpoint.
#[derive(Serialize)]
struct LoginRequest<'a> {
    grant_type: &'a str,
    #[serde(rename = "openId")]
    open_id: &'a str,
    account_type: &'a str,
}

/// Partial shape of the login response we care about.
#[derive(Deserialize)]
struct LoginResponse {
    code: i64,
    #[serde(alias = "msg", default)]
    message: Option<String>,
    data: Option<LoginData>,
}

#[derive(Deserialize)]
struct LoginData {
    access_token: String,
}

/// Exchange an `api_key` (openId) for an access token and return a [`Credentials`] value.
///
/// This performs a blocking `POST` to the platform's login endpoint.
pub fn login(api_key: &str, config: &TianyanConfig) -> Result<Credentials, TianyanError> {
    login_with_timeout(api_key, config, std::time::Duration::from_secs(60))
}

pub(crate) fn login_with_timeout(
    api_key: &str,
    config: &TianyanConfig,
    timeout: std::time::Duration,
) -> Result<Credentials, TianyanError> {
    let url = format!("{}{}", config.base_url(), LOGIN_PATH);
    let body = LoginRequest {
        grant_type: "openId",
        open_id: api_key,
        account_type: "member",
    };

    let client = reqwest::blocking::Client::new();
    let resp: LoginResponse = client
        .post(&url)
        .timeout(timeout)
        .form(&body)
        .send()
        .map_err(TianyanError::Http)?
        .error_for_status()
        .map_err(TianyanError::Http)?
        .json()
        .map_err(TianyanError::Http)?;

    if resp.code != 0 {
        return Err(TianyanError::Auth(resp.message.unwrap_or_else(|| {
            format!("login failed with code {}", resp.code)
        })));
    }

    let data = resp
        .data
        .ok_or_else(|| TianyanError::Auth("login response missing 'data' field".to_string()))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(Credentials {
        api_key: api_key.to_string(),
        access_token: data.access_token,
        token_obtained_at: now,
    })
}

/// Serialise `credentials` to JSON and write them to `path`.
///
/// Parent directories are created automatically.
pub fn save_credentials(credentials: &Credentials, path: &Path) -> Result<(), TianyanError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(credentials)?;
    fs::write(path, json)?;

    // Restrict the credentials file to the owner only (Unix: mode 0o600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Read and deserialise credentials from `path`.
pub fn load_credentials(path: &Path) -> Result<Credentials, TianyanError> {
    let json = fs::read_to_string(path).map_err(|e| {
        TianyanError::Auth(format!(
            "could not read credentials from {}: {}",
            path.display(),
            e
        ))
    })?;
    let creds: Credentials = serde_json::from_str(&json)?;
    Ok(creds)
}

#[cfg(test)]
#[path = "auth_test.rs"]
mod tests;
