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

//! Python bindings for [`cqlib_tianyan::TianyanConfig`].
//!
//! # Example
//!
//! ```python
//! from cqlib_tianyan import TianyanConfig
//!
//! # Use defaults (official production endpoint, credentials saved to disk)
//! cfg = TianyanConfig()
//!
//! # Custom domain
//! cfg = TianyanConfig.custom("my-domain.com")
//!
//! # Builder-style configuration
//! cfg = (TianyanConfig()
//!        .with_save_credentials(False)
//!        .with_auto_refresh(False)
//!        .with_credentials_path("/tmp/creds.json"))
//! ```

use cqlib_tianyan::config::TianyanConfig;
use pyo3::prelude::*;

/// Runtime configuration for the Tianyan platform client.
///
/// All parameters are optional and default to the official production values.
///
/// # Example
///
/// ```python
/// from cqlib_tianyan import TianyanConfig
///
/// # Official production endpoint, credentials saved to disk (defaults)
/// cfg = TianyanConfig()
///
/// # Custom domain
/// cfg = TianyanConfig(domain="my-domain.com")
///
/// # Disable credential persistence and auto-refresh
/// cfg = TianyanConfig(save_credentials=False, auto_refresh=False)
///
/// # Full customisation
/// cfg = TianyanConfig(
///     domain="my-domain.com",
///     save_credentials=False,
///     auto_refresh=False,
///     credentials_path="/tmp/tianyan_creds.json",
/// )
/// ```
#[pyclass(name = "TianyanConfig", module = "cqlib_tianyan", skip_from_py_object)]
#[derive(Clone)]
pub struct PyTianyanConfig {
    pub(crate) inner: TianyanConfig,
}

impl From<TianyanConfig> for PyTianyanConfig {
    fn from(inner: TianyanConfig) -> Self {
        Self { inner }
    }
}

impl From<PyTianyanConfig> for TianyanConfig {
    fn from(value: PyTianyanConfig) -> Self {
        value.inner
    }
}

#[pymethods]
impl PyTianyanConfig {
    /// Create a Tianyan configuration.
    ///
    /// # Arguments
    /// * `domain` - Platform hostname (default: `"qc.zdxlz.com"`).
    /// * `scheme` - URL scheme (default: `"https"`; use `"http"` for some test envs).
    /// * `save_credentials` - Whether to persist credentials to disk (default: `True`).
    /// * `auto_refresh` - Whether to re-login automatically on token expiry (default: `True`).
    /// * `credentials_path` - File path for the JSON credentials file
    ///   (default: `~/.cqlib/tianyan/credentials.json`).
    #[new]
    #[pyo3(signature = (
        domain = None,
        scheme = None,
        save_credentials = None,
        auto_refresh = None,
        credentials_path = None,
    ))]
    fn new(
        domain: Option<String>,
        scheme: Option<String>,
        save_credentials: Option<bool>,
        auto_refresh: Option<bool>,
        credentials_path: Option<String>,
    ) -> Self {
        let mut cfg = match domain {
            Some(d) => TianyanConfig::custom(d),
            None => TianyanConfig::default(),
        };
        if let Some(s) = scheme {
            cfg = cfg.with_scheme(s);
        }
        if let Some(v) = save_credentials {
            cfg = cfg.with_save_credentials(v);
        }
        if let Some(v) = auto_refresh {
            cfg = cfg.with_auto_refresh(v);
        }
        if let Some(p) = credentials_path {
            cfg = cfg.with_credentials_path(std::path::PathBuf::from(p));
        }
        cfg.into()
    }

    /// The base URL constructed from scheme + domain.
    #[getter]
    fn base_url(&self) -> String {
        self.inner.base_url()
    }

    /// The configured hostname.
    #[getter]
    fn domain(&self) -> String {
        self.inner.domain.clone()
    }

    /// The URL scheme (`"https"` or `"http"`).
    #[getter]
    fn scheme(&self) -> String {
        self.inner.scheme.clone()
    }

    /// Whether credentials are saved to disk after login/refresh.
    #[getter]
    fn save_credentials(&self) -> bool {
        self.inner.save_credentials
    }

    /// Whether an expired token triggers an automatic re-login.
    #[getter]
    fn auto_refresh(&self) -> bool {
        self.inner.auto_refresh
    }

    /// The credentials file path.
    #[getter]
    fn credentials_path(&self) -> String {
        self.inner.credentials_path.to_string_lossy().into_owned()
    }

    fn __repr__(&self) -> String {
        format!(
            "TianyanConfig(domain='{}', save_credentials={}, auto_refresh={})",
            self.inner.domain, self.inner.save_credentials, self.inner.auto_refresh,
        )
    }
}
