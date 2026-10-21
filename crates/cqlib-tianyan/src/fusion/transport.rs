//! Fusion transport on the existing authenticated TianyanClient.
use super::FusionError;
use crate::client::TianyanClient;
use reqwest::{
    Method, StatusCode,
    blocking::{Response, multipart},
};
use serde_json::Value;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(super) enum Body<'a> {
    Empty,
    Json(&'a Value),
    Upload(&'a Path),
}

impl TianyanClient {
    /// Only reads retry transient failures. Mutations may refresh after a 401
    /// once, but never replay after a timeout, connection loss, 429 or 5xx.
    pub(super) fn fusion_request(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Body<'_>,
        timeout: Duration,
    ) -> Result<Response, FusionError> {
        let started = Instant::now();
        let read_only = method == Method::GET;
        let mut token = self.current_token_timeout(timeout)?;
        let mut refreshed = false;
        let mut retries = 0;
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(FusionError {
                    kind: "timeout".into(),
                    ..FusionError::invalid("fusion request timed out")
                });
            }
            let mut request = self
                .http
                .request(
                    method.clone(),
                    format!("{}{}", self.config.base_url(), path),
                )
                .headers(Self::auth_headers(&token))
                .query(query)
                .timeout(remaining);
            request = match &body {
                Body::Empty => request,
                Body::Json(value) => request.json(value),
                Body::Upload(path) => {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("task.zip");
                    let file = std::fs::File::open(path)?;
                    let part = multipart::Part::reader_with_length(file, path.metadata()?.len())
                        .file_name(name.to_owned())
                        .mime_str("application/zip")?;
                    request.multipart(multipart::Form::new().part("file", part))
                }
            };
            match request.send() {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::UNAUTHORIZED && !refreshed {
                        let remaining = timeout.saturating_sub(started.elapsed());
                        if remaining.is_zero() {
                            return Err(FusionError {
                                kind: "timeout".into(),
                                ..FusionError::invalid("fusion authentication timed out")
                            });
                        }
                        token = self.refresh_token_timeout(remaining)?;
                        refreshed = true;
                        continue;
                    }
                    if read_only
                        && retries < 2
                        && (status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS)
                    {
                        let delay = response
                            .headers()
                            .get("Retry-After")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(1 << retries)
                            .min(30);
                        retries += 1;
                        std::thread::sleep(
                            Duration::from_secs(delay)
                                .min(timeout.saturating_sub(started.elapsed())),
                        );
                        continue;
                    }
                    if !status.is_success() {
                        let text = response.text()?;
                        let payload = serde_json::from_str(&text).unwrap_or(Value::Null);
                        return Err(FusionError::response(status.as_u16(), &payload, &text));
                    }
                    return Ok(response);
                }
                Err(error) if read_only && retries < 2 => {
                    let _ = error;
                    std::thread::sleep(
                        Duration::from_secs(1 << retries)
                            .min(timeout.saturating_sub(started.elapsed())),
                    );
                    retries += 1;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}
