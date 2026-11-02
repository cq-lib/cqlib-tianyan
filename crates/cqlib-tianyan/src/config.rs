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
// Modified to add the filterable quantum computer query endpoint.

//! Configuration for the Tianyan quantum cloud platform.
//!
//! [`TianyanConfig`] holds the base URL and the local path used to store
//! authentication credentials.  In most cases callers can use [`TianyanConfig::default`]
//! which points to the official production environment (`https://qc.zdxlz.com`).

use std::path::PathBuf;

/// Login / token acquisition endpoint.
pub const LOGIN_PATH: &str = "/qccp-auth/oauth2/sdk/opnId";

/// Device (quantum computer) list endpoint.
pub const DEVICE_LIST_PATH: &str = "/qccp-quantum/sdk/quantumComputer/list";

/// Filterable quantum computer query endpoint.
pub const DEVICE_QUERY_PATH: &str = "/qccp-quantum/experiments/quantum/computer/by/user";

/// Batch circuit submission endpoint (max 50 circuits per call).
pub const SUBMIT_PATH: &str = "/qccp-quantum/sdk/experiment/submit";

/// Result query endpoint.
pub const QUERY_RESULT_PATH: &str = "/qccp-quantum/sdk/experiment/result/find";

/// Device calibration configuration download endpoint (append `/{machine}`).
pub const DOWNLOAD_CONFIG_PATH: &str = "/qccp-quantum/sdk/experiment/download/config";

const DEFAULT_SCHEME: &str = "https";
const DEFAULT_DOMAIN: &str = "qc.zdxlz.com";

/// Runtime configuration for the Tianyan platform client.
///
/// Use [`TianyanConfig::default`] for the official production environment,
/// or chain builder methods to customise behaviour:
///
/// ```rust,no_run
/// use cqlib_tianyan::config::TianyanConfig;
///
/// let cfg = TianyanConfig::default()
///     .with_save_credentials(false)   // in-memory only, do not write to disk
///     .with_auto_refresh(false);      // fail rather than re-login on expiry
/// ```
#[derive(Debug, Clone)]
pub struct TianyanConfig {
    /// URL scheme, almost always `"https"`.
    pub scheme: String,
    /// Hostname of the platform, e.g. `"qc.zdxlz.com"`.
    pub domain: String,
    /// Local path for the JSON credentials file.
    ///
    /// Defaults to:
    /// - **Unix / macOS**: `~/.cqlib/tianyan/credentials.json`
    /// - **Windows**: `%APPDATA%\cqlib\tianyan\credentials.json`
    pub credentials_path: PathBuf,
    /// Whether to persist credentials to [`credentials_path`](Self::credentials_path) after
    /// a successful login or token refresh.  Default: `true`.
    pub save_credentials: bool,
    /// Whether to automatically re-login when the stored token has expired.
    /// If `false`, an expired token causes [`TianyanError::Auth`].  Default: `true`.
    pub auto_refresh: bool,
}

impl Default for TianyanConfig {
    fn default() -> Self {
        Self {
            scheme: DEFAULT_SCHEME.to_string(),
            domain: DEFAULT_DOMAIN.to_string(),
            credentials_path: default_credentials_path(),
            save_credentials: true,
            auto_refresh: true,
        }
    }
}

impl TianyanConfig {
    /// Create a config pointing to a custom domain (same credentials path).
    pub fn custom(domain: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            ..Self::default()
        }
    }

    /// Set whether credentials are saved to disk after login/refresh (default: `true`).
    pub fn with_save_credentials(mut self, save: bool) -> Self {
        self.save_credentials = save;
        self
    }

    /// Set whether an expired token triggers an automatic re-login (default: `true`).
    pub fn with_auto_refresh(mut self, auto_refresh: bool) -> Self {
        self.auto_refresh = auto_refresh;
        self
    }

    /// Override the credentials file path.
    pub fn with_credentials_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.credentials_path = path.into();
        self
    }

    /// Compose the full base URL, e.g. `"https://qc.zdxlz.com"`.
    pub fn base_url(&self) -> String {
        format!("{}://{}", self.scheme, self.domain)
    }
}

/// Returns the default credentials file path for the current platform.
///
/// | Platform | Path |
/// |----------|------|
/// | Unix / macOS | `~/.cqlib/tianyan/credentials.json` |
/// | Windows | `%APPDATA%\cqlib\tianyan\credentials.json` |
fn default_credentials_path() -> PathBuf {
    #[cfg(windows)]
    {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("cqlib")
            .join("tianyan")
            .join("credentials.json")
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cqlib")
            .join("tianyan")
            .join("credentials.json")
    }
}

#[cfg(test)]
#[path = "config_test.rs"]
mod tests;
