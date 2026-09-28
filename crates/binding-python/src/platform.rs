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
// Modified to accept a calibration policy when submitting circuits.

//! Python bindings for [`TianyanPlatform`] — the top-level entry point.
//!
//! Configuration parameters (`domain`, `save_credentials`, `auto_refresh`,
//! `credentials_path`) are accepted directly as keyword arguments on
//! `login()` and `from_credentials()`, so callers never need to construct
//! a separate config object.
//!
//! # Example
//!
//! ```python
//! from cqlib_tianyan import TianyanPlatform
//!
//! # First-time login — credentials saved to ~/.cqlib/tianyan/credentials.json
//! platform = TianyanPlatform.login("your_api_key")
//!
//! # In-memory only (no disk writes)
//! platform = TianyanPlatform.login("your_api_key", save_credentials=False)
//!
//! # Subsequent runs — reload from disk, auto-refresh if expired
//! platform = TianyanPlatform.from_credentials()
//!
//! # Custom domain
//! platform = TianyanPlatform.login("your_api_key", domain="my-cloud.example.com")
//!
//! # Discover backends
//! for b in platform.list_backends():
//!     print(b.name, b.status)
//!
//! # Submit circuits
//! task = platform.submit(["H Q1\nM Q1"], shots=1000, device_name="tianyan-287")
//! results = task.wait(timeout=120.0)
//! ```

use crate::backend::{CalibrationModeInput, PyTianyanBackend};
use crate::error::IntoPyResult;
use crate::task::PyTaskHandle;
use cqlib_tianyan::TianyanPlatform;
use cqlib_tianyan::config::TianyanConfig;
use cqlib_tianyan::task::CalibrationMode;
use pyo3::prelude::*;

/// Build a `TianyanConfig` from optional keyword arguments.
fn build_config(
    domain: Option<String>,
    save_credentials: Option<bool>,
    auto_refresh: Option<bool>,
    credentials_path: Option<String>,
) -> TianyanConfig {
    let mut cfg = match domain {
        Some(d) => TianyanConfig::custom(d),
        None => TianyanConfig::default(),
    };
    if let Some(v) = save_credentials {
        cfg = cfg.with_save_credentials(v);
    }
    if let Some(v) = auto_refresh {
        cfg = cfg.with_auto_refresh(v);
    }
    if let Some(p) = credentials_path {
        cfg = cfg.with_credentials_path(std::path::PathBuf::from(p));
    }
    cfg
}

/// Synchronous client for the Tianyan quantum cloud platform.
///
/// All methods use blocking I/O. The GIL is released during `wait()` and
/// `wait_raw()` polling so other Python threads remain active.
#[pyclass(name = "TianyanPlatform", module = "cqlib_tianyan")]
pub struct PyTianyanPlatform {
    inner: TianyanPlatform,
    /// Stored for display in `__repr__`.
    base_url: String,
}

#[pymethods]
impl PyTianyanPlatform {
    /// Authenticate with an API key and return a platform client.
    ///
    /// Credentials are saved to `~/.cqlib/tianyan/credentials.json` by default
    /// so subsequent calls can use `from_credentials()`.
    ///
    /// # Arguments
    /// * `api_key` - Your Tianyan platform API key (openId).
    /// * `domain` - Platform hostname (default: `"qc.zdxlz.com"`).
    /// * `save_credentials` - Persist credentials to disk (default: `True`).
    /// * `auto_refresh` - Re-login automatically on token expiry (default: `True`).
    /// * `credentials_path` - Custom path for the credentials JSON file.
    ///
    /// # Raises
    /// `TianyanError` on authentication failure or network error.
    #[staticmethod]
    #[pyo3(signature = (
        api_key,
        *,
        domain = None,
        save_credentials = None,
        auto_refresh = None,
        credentials_path = None,
    ))]
    fn login(
        py: Python<'_>,
        api_key: &str,
        domain: Option<String>,
        save_credentials: Option<bool>,
        auto_refresh: Option<bool>,
        credentials_path: Option<String>,
    ) -> PyResult<Self> {
        let cfg = build_config(domain, save_credentials, auto_refresh, credentials_path);
        let base_url = cfg.base_url();
        TianyanPlatform::login_with_config(api_key, cfg)
            .map_py_err(py)
            .map(|inner| Self { inner, base_url })
    }

    /// Load previously saved credentials from disk and return a platform client.
    ///
    /// If the stored access token has expired, re-logs in automatically
    /// using the saved API key (requires `auto_refresh=True`, the default).
    ///
    /// # Arguments
    /// * `domain` - Platform hostname (default: `"qc.zdxlz.com"`).
    /// * `save_credentials` - Persist refreshed credentials to disk (default: `True`).
    /// * `auto_refresh` - Re-login automatically on token expiry (default: `True`).
    /// * `credentials_path` - Custom path for the credentials JSON file.
    ///
    /// # Raises
    /// `TianyanError` if the credentials file is missing or the token has
    /// expired and `auto_refresh=False`.
    #[staticmethod]
    #[pyo3(signature = (
        *,
        domain = None,
        save_credentials = None,
        auto_refresh = None,
        credentials_path = None,
    ))]
    fn from_credentials(
        py: Python<'_>,
        domain: Option<String>,
        save_credentials: Option<bool>,
        auto_refresh: Option<bool>,
        credentials_path: Option<String>,
    ) -> PyResult<Self> {
        let cfg = build_config(domain, save_credentials, auto_refresh, credentials_path);
        let base_url = cfg.base_url();
        TianyanPlatform::from_credentials_with_config(cfg)
            .map_py_err(py)
            .map(|inner| Self { inner, base_url })
    }

    /// Fetch the full list of available quantum backends from the platform.
    ///
    /// # Returns
    /// List of `TianyanBackend` objects.
    fn list_backends(&self, py: Python<'_>) -> PyResult<Vec<PyTianyanBackend>> {
        self.inner
            .list_backends()
            .map_py_err(py)
            .map(|bs| bs.into_iter().map(PyTianyanBackend::from).collect())
    }

    /// Return the backend with the given name.
    ///
    /// # Arguments
    /// * `name` - Case-sensitive backend identifier (e.g. `"tianyan-287"`).
    ///
    /// # Raises
    /// `TianyanError` if no backend with that name exists.
    fn get_backend(&self, py: Python<'_>, name: &str) -> PyResult<PyTianyanBackend> {
        self.inner
            .get_backend(name)
            .map_py_err(py)
            .map(PyTianyanBackend::from)
    }

    /// Submit circuits without fetching a backend handle first.
    ///
    /// Equivalent to `platform.get_backend(device_name).run(circuits, shots,
    /// calibration_mode=calibration_mode)`,
    /// but skips the extra device-list network round-trip.
    ///
    /// # Arguments
    /// * `circuits` - List of QCIS circuit strings.
    /// * `shots` - Number of measurement shots per circuit.
    /// * `device_name` - Target backend identifier.
    /// * `calibration_mode` - Keyword-only policy: `"auto"` (default), `"enabled"`,
    ///   or `"disabled"`, as a string or `CalibrationMode` object.
    ///   Only superconducting devices and simulators can submit tasks;
    ///   `"enabled"` rejects non-superconducting devices before submission.
    #[pyo3(
        signature = (circuits, shots, device_name, *, calibration_mode = CalibrationModeInput(CalibrationMode::Auto)),
        text_signature = "($self, circuits, shots, device_name, *, calibration_mode='auto')"
    )]
    fn submit(
        &self,
        py: Python<'_>,
        circuits: Vec<String>,
        shots: usize,
        device_name: &str,
        calibration_mode: CalibrationModeInput,
    ) -> PyResult<PyTaskHandle> {
        let circuit_inputs: Vec<cqlib_tianyan::device::CircuitInput> =
            circuits.into_iter().map(|s| s.into()).collect();
        self.inner
            .submit_with_mode(circuit_inputs, shots, device_name, calibration_mode.0)
            .map_py_err(py)
            .map(PyTaskHandle::from)
    }

    fn __repr__(&self) -> String {
        format!("TianyanPlatform(url='{}')", self.base_url)
    }
}
