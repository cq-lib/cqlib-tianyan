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

//! Top-level entry point for the Tianyan quantum cloud platform client.
//!
//! [`TianyanPlatform`] is the primary struct callers interact with.  It bundles
//! authentication, device discovery, and circuit submission behind a clean API.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use cqlib_tianyan::TianyanPlatform;
//! use std::time::Duration;
//!
//! fn main() -> Result<(), cqlib_tianyan::TianyanError> {
//!     // First-time login
//!     // Credentials are saved to ~/.cqlib/tianyan/credentials.json
//!     let api_key = std::env::var("TIANYAN_API_KEY").expect("set TIANYAN_API_KEY");
//!     let platform = TianyanPlatform::login(&api_key)?;
//!
//!     // Subsequent runs
//!     // Reload from disk; re-login automatically if the token has expired.
//!     // let platform = TianyanPlatform::from_credentials()?;
//!
//!     // Backend discovery
//!     let backends = platform.list_backends()?;
//!     for b in &backends {
//!         println!("{} ({:?})", b.name, b.status);
//!     }
//!
//!     // Submit via a backend handle
//!     // Bell state: H Q1 + CNOT(Q1→Q8), decomposed as H·CZ·H on target
//!     let backend = platform.get_backend("tianyan-287")?;
//!     let task = backend.run(
//!         vec!["H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8".into()],
//!         1000,
//!     )?;
//!
//!     // Poll for results
//!     let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
//!     for r in &results {
//!         println!("counts: {:?}", r.counts());
//!     }
//!     Ok(())
//! }
//! ```

use crate::auth::{self, save_credentials};
use crate::client::TianyanClient;
use crate::config::TianyanConfig;
use crate::device::{self, CircuitInput, TianyanBackend};
use crate::error::TianyanError;
use crate::task::{CalibrationMode, TaskHandle};
use std::sync::Arc;

/// Synchronous client for the Tianyan quantum cloud platform.
///
/// All methods use blocking I/O, making it straightforward to integrate with
/// Python (PyO3), C (FFI), or Java (JNI) bindings.
pub struct TianyanPlatform {
    pub(crate) client: Arc<TianyanClient>,
}

impl TianyanPlatform {
    /// Authenticate with `api_key`.
    ///
    /// By default credentials are saved to `~/.cqlib/tianyan/credentials.json`
    /// (platform-appropriate path) so subsequent calls can use [`from_credentials`](Self::from_credentials).
    ///
    /// Customise saving behaviour via [`TianyanConfig`]:
    /// ```rust,no_run
    /// use cqlib_tianyan::{TianyanPlatform, config::TianyanConfig};
    ///
    /// // In-memory only – credentials are never written to disk
    /// let cfg = TianyanConfig::default().with_save_credentials(false);
    /// let platform = TianyanPlatform::login_with_config("my-api-key", cfg)?;
    /// # Ok::<(), cqlib_tianyan::TianyanError>(())
    /// ```
    pub fn login(api_key: &str) -> Result<Self, TianyanError> {
        Self::login_with_config(api_key, TianyanConfig::default())
    }

    /// Like [`login`](Self::login) but with a custom [`TianyanConfig`].
    pub fn login_with_config(api_key: &str, config: TianyanConfig) -> Result<Self, TianyanError> {
        let creds = auth::login(api_key, &config)?;
        if config.save_credentials {
            save_credentials(&creds, &config.credentials_path)?;
        }
        Ok(Self {
            client: Arc::new(TianyanClient::new(config, creds)?),
        })
    }

    /// Load previously saved credentials from disk.
    ///
    /// If `config.auto_refresh` is `true` (the default) and the stored access
    /// token has expired, the library re-logs in using the saved `api_key`
    /// and updates the credentials file.
    ///
    /// Set `auto_refresh = false` to get a hard error on expiry instead:
    /// ```rust,no_run
    /// use cqlib_tianyan::{TianyanPlatform, config::TianyanConfig};
    ///
    /// let cfg = TianyanConfig::default().with_auto_refresh(false);
    /// let platform = TianyanPlatform::from_credentials_with_config(cfg)?;
    /// # Ok::<(), cqlib_tianyan::TianyanError>(())
    /// ```
    pub fn from_credentials() -> Result<Self, TianyanError> {
        Self::from_credentials_with_config(TianyanConfig::default())
    }

    /// Like [`from_credentials`](Self::from_credentials) but with a custom
    /// [`TianyanConfig`].
    pub fn from_credentials_with_config(config: TianyanConfig) -> Result<Self, TianyanError> {
        let mut creds = auth::load_credentials(&config.credentials_path)?;

        if creds.is_token_expired() {
            if !config.auto_refresh {
                return Err(TianyanError::Auth(
                    "access token has expired; set auto_refresh=true or call login() again"
                        .to_string(),
                ));
            }
            creds = auth::login(&creds.api_key, &config)?;
            if config.save_credentials {
                save_credentials(&creds, &config.credentials_path)?;
            }
        }

        Ok(Self {
            client: Arc::new(TianyanClient::new(config, creds)?),
        })
    }

    /// Fetch the full list of quantum backends from the platform.
    pub fn list_backends(&self) -> Result<Vec<TianyanBackend>, TianyanError> {
        device::list_backends(self.client.clone())
    }

    /// Return the backend with the given `name` (case-sensitive `computerCode`).
    ///
    /// # Errors
    /// Returns [`TianyanError::DeviceNotFound`] if no backend with that name exists.
    pub fn get_backend(&self, name: &str) -> Result<TianyanBackend, TianyanError> {
        let backends = self.list_backends()?;
        backends
            .into_iter()
            .find(|d| d.name == name)
            .ok_or_else(|| TianyanError::DeviceNotFound(name.to_string()))
    }

    /// Submit circuits directly without first fetching a [`TianyanBackend`] handle.
    ///
    /// This is equivalent to calling `platform.get_backend(device_name)?.run(circuits, shots)`,
    /// but skips the extra network round-trip for the device lookup.
    /// Only superconducting devices and simulators can submit tasks.
    pub fn submit(
        &self,
        circuits: Vec<CircuitInput>,
        shots: usize,
        device_name: &str,
    ) -> Result<TaskHandle, TianyanError> {
        self.submit_with_mode(circuits, shots, device_name, CalibrationMode::Auto)
    }

    /// Submit directly with an explicit calibration policy, without a device lookup.
    /// Device capabilities are checked before any circuits are submitted.
    pub fn submit_with_mode(
        &self,
        circuits: Vec<CircuitInput>,
        shots: usize,
        device_name: &str,
        calibration_mode: CalibrationMode,
    ) -> Result<TaskHandle, TianyanError> {
        TaskHandle::submit(
            self.client.clone(),
            circuits,
            shots,
            device_name,
            calibration_mode,
        )
    }
}
