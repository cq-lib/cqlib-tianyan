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

//! Quantum backend representation and listing for the Tianyan platform.
//!
//! [`TianyanBackend`] is returned by the platform's device-list endpoint.  It carries
//! enough information to decide which backend to use and provides a [`TianyanBackend::run`]
//! shortcut that submits circuits directly on that backend.
//!
//! # Circuit input
//!
//! [`CircuitInput`] is a flexible enum that accepts either:
//! - a QCIS string (`&str` / `String`) — converted via `From` impls, or
//! - a [`cqlib_core::circuit::Circuit`] object — automatically serialised to QCIS.

use crate::client::TianyanClient;
use crate::config::DEVICE_LIST_PATH;
use crate::device_config::{self, ReadoutCalibrationData};
use crate::error::TianyanError;
use crate::task::{CalibrationMode, TaskHandle};
use cqlib_core::circuit::Circuit;
use cqlib_core::device::Device;
use cqlib_core::ir::qcis::dumps;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

/// A single circuit ready for submission, either as a raw QCIS string or a
/// `cqlib-core` [`Circuit`] object.
///
/// The `From` trait is implemented for `String`, `&str`, and `Circuit` so callers
/// can write `"H Q0\nM Q0".into()` or `my_circuit.into()` without boilerplate.
pub enum CircuitInput {
    /// A pre-serialised QCIS string.
    Qcis(String),
    /// A circuit IR object; converted to QCIS just before submission.
    ///
    /// Boxed to keep the enum size small (`Circuit` is a large struct).
    Circuit(Box<Circuit>),
}

impl CircuitInput {
    /// Convert the variant into a QCIS string, returning an error if conversion fails.
    pub fn into_qcis(self) -> Result<String, TianyanError> {
        match self {
            CircuitInput::Qcis(s) => Ok(s),
            CircuitInput::Circuit(c) => {
                dumps(&c).map_err(|e| TianyanError::CircuitConversion(e.to_string()))
            }
        }
    }
}

impl From<String> for CircuitInput {
    fn from(s: String) -> Self {
        CircuitInput::Qcis(s)
    }
}

impl From<&str> for CircuitInput {
    fn from(s: &str) -> Self {
        CircuitInput::Qcis(s.to_string())
    }
}

impl From<Circuit> for CircuitInput {
    fn from(c: Circuit) -> Self {
        CircuitInput::Circuit(Box::new(c))
    }
}

/// Operational status of a quantum device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Device is online and accepting jobs.
    Running,
    /// Device is undergoing calibration; submissions may queue.
    Calibration,
    /// Device is temporarily unavailable for maintenance.
    UnderMaintenance,
    /// Device is offline.
    OffLine,
    /// Device is undergoing an upgrade.
    Upgrading,
    /// An unknown status code was returned by the API.
    Unknown(i64),
}

impl DeviceStatus {
    fn from_code(code: i64) -> Self {
        match code {
            0 => DeviceStatus::Running,
            1 => DeviceStatus::Calibration,
            2 => DeviceStatus::UnderMaintenance,
            3 => DeviceStatus::OffLine,
            4 => DeviceStatus::Upgrading,
            other => DeviceStatus::Unknown(other),
        }
    }
}

/// Backend technology, classified locally from the case-sensitive machine code.
///
/// The device-list API does not provide this field. Codes starting with `tianyan_`
/// are simulators, `tianyan-p` are photonic, and `tianyan-ion` are ion traps.
/// Other codes starting with `tianyan` are superconducting. Devices without the
/// `tianyan` prefix are excluded from the backend list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Superconducting,
    Photonic,
    IonTrap,
    Simulator,
}

impl DeviceType {
    pub fn from_code(code: &str) -> Option<Self> {
        if !code.starts_with("tianyan") {
            return None;
        }
        Some(if code.starts_with("tianyan_") {
            Self::Simulator
        } else if code.starts_with("tianyan-p") {
            Self::Photonic
        } else if code.starts_with("tianyan-ion") {
            Self::IonTrap
        } else {
            Self::Superconducting
        })
    }
}

/// Pricing model of a quantum device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceToll {
    Free,
    Paid,
    Unknown(i64),
}

impl DeviceToll {
    fn from_code(code: i64) -> Self {
        match code {
            1 => DeviceToll::Free,
            2 => DeviceToll::Paid,
            other => DeviceToll::Unknown(other),
        }
    }
}

#[derive(Deserialize)]
struct RawDevice {
    /// Unique machine identifier used as `computerCode` in submission requests.
    code: String,
    /// Human-readable display name.
    name: Option<String>,
    /// Operational status code (0=running, 1=calibration, 2=maintenance, 3=offline, 4=upgrading).
    status: i64,
    /// Pricing tier code (1=free, 2=paid).
    #[serde(rename = "isToll")]
    is_toll: Option<i64>,
}

/// Cached device calibration data (loaded lazily on first access).
#[derive(Clone)]
struct CachedConfig {
    device: Device,
    calibration: Option<ReadoutCalibrationData>,
}

/// A quantum computing backend available on the Tianyan cloud platform.
///
/// Use [`TianyanBackend::is_available`] to check the backend's running status.
/// Task submission additionally requires a superconducting device or simulator.
pub struct TianyanBackend {
    /// The machine code used as `computerCode` when submitting jobs.
    pub name: String,
    /// User-friendly display name.
    pub display_name: String,
    /// Backend technology, classified locally from the machine code.
    pub device_type: DeviceType,
    /// Current operational status.
    pub status: DeviceStatus,
    /// Pricing model.
    pub toll: DeviceToll,
    /// Shared HTTP client used by [`TianyanBackend::run`].
    pub(crate) client: Arc<TianyanClient>,
    /// Lazily-loaded device configuration (topology + calibration).
    cached_config: Mutex<Option<CachedConfig>>,
}

impl Clone for TianyanBackend {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            display_name: self.display_name.clone(),
            device_type: self.device_type,
            status: self.status.clone(),
            toll: self.toll.clone(),
            client: self.client.clone(),
            cached_config: Mutex::new(
                self.cached_config
                    .lock()
                    .expect("cached_config mutex poisoned during clone")
                    .clone(),
            ),
        }
    }
}

impl std::fmt::Debug for TianyanBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TianyanBackend")
            .field("name", &self.name)
            .field("display_name", &self.display_name)
            .field("device_type", &self.device_type)
            .field("status", &self.status)
            .field("toll", &self.toll)
            .finish()
    }
}

impl TianyanBackend {
    /// Build a backend from a Tianyan API record, excluding non-Tianyan codes.
    fn from_raw(raw: RawDevice, client: Arc<TianyanClient>) -> Option<Self> {
        let device_type = DeviceType::from_code(&raw.code)?;
        Some(Self {
            display_name: raw.name.clone().unwrap_or_else(|| raw.code.clone()),
            device_type,
            name: raw.code,
            status: DeviceStatus::from_code(raw.status),
            toll: DeviceToll::from_code(raw.is_toll.unwrap_or(0)),
            client,
            cached_config: Mutex::new(None),
        })
    }

    /// Returns `true` when the device is [`DeviceStatus::Running`].
    pub fn is_available(&self) -> bool {
        self.status == DeviceStatus::Running
    }

    /// Ensure the config is loaded, then run a closure on the cached data.
    fn with_config<F, R>(&self, f: F) -> Result<R, TianyanError>
    where
        F: FnOnce(&CachedConfig) -> R,
    {
        let mut guard = self
            .cached_config
            .lock()
            .map_err(|_| TianyanError::InvalidInput("cached_config mutex poisoned".into()))?;
        if guard.is_none() {
            let (device, calibration) =
                device_config::download_device_config_full(&self.client, &self.name)?;
            *guard = Some(CachedConfig {
                device,
                calibration,
            });
        }
        Ok(f(guard.as_ref().expect("guard was just populated")))
    }

    /// Access the device configuration with a closure.
    ///
    /// This method downloads (or returns cached) calibration configuration and
    /// provides access to the underlying [`cqlib_core::device::Device`] which
    /// contains topology, qubit properties, gate errors, and readout fidelities.
    ///
    /// Only superconducting backends support configuration access. Other device
    /// types return an error before any download request.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use cqlib_tianyan::TianyanPlatform;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let platform = TianyanPlatform::login("test_key")?;
    /// # let backend = platform.get_backend("tianyan-287")?;
    /// backend.with_device(|device| {
    ///     println!("Qubits: {}", device.qubits().count());
    ///     println!("Couplings: {}", device.topology().num_couplings());
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_device<F, R>(&self, f: F) -> Result<R, TianyanError>
    where
        F: FnOnce(&Device) -> R,
    {
        self.with_config(|c| f(&c.device))
    }

    /// Return the total number of physical qubits in the backend configuration.
    ///
    /// This method downloads the backend configuration on first use and reuses the
    /// cached configuration afterwards. Disabled qubits are included in this count;
    /// use [`with_device`](Self::with_device) and inspect `device.topology()` when
    /// you need the currently available topology qubit count.
    /// Only superconducting backends support this operation.
    pub fn num_qubits(&self) -> Result<usize, TianyanError> {
        self.with_device(|device| device.qubits().count())
    }

    /// Download (or return cached) readout calibration data suitable for
    /// measurement error mitigation.
    ///
    /// Returns `None` if the backend's config does not contain the required
    /// readout fidelity arrays. Only superconducting backends support this operation.
    pub fn readout_calibration_data(&self) -> Result<Option<ReadoutCalibrationData>, TianyanError> {
        self.with_config(|c| c.calibration.clone())
    }

    /// Submit one or more circuits on this backend.
    ///
    /// By default, [`wait`](crate::task::TaskHandle::wait) on the returned handle will
    /// apply readout error mitigation on superconducting devices if calibration data is available
    /// ([`CalibrationMode::Auto`]).  Pass a custom mode via
    /// [`run_with_mode`](Self::run_with_mode) to override this behaviour.
    /// Only superconducting backends and simulators can submit tasks.
    ///
    /// ```rust,ignore
    /// # use cqlib_tianyan::{TianyanPlatform, device::CircuitInput};
    /// # use std::time::Duration;
    /// let platform = TianyanPlatform::login("your_api_key")?;
    /// let backend = platform.get_backend("tianyan-287")?;
    ///
    /// // QCIS string — results will be readout-calibrated by default
    /// let task = backend.run(vec!["H Q0\nCZ Q0 Q1\nM Q0 Q1".into()], 1000)?;
    /// ```
    pub fn run(
        &self,
        circuits: Vec<CircuitInput>,
        shots: usize,
    ) -> Result<TaskHandle, TianyanError> {
        self.run_with_mode(circuits, shots, CalibrationMode::Auto)
    }

    /// Like [`run`](Self::run) but with an explicit [`CalibrationMode`].
    /// `Enabled` is rejected for non-superconducting devices before submission.
    ///
    /// ```rust,ignore
    /// use cqlib_tianyan::task::CalibrationMode;
    ///
    /// // Never apply readout calibration
    /// let task = backend.run_with_mode(circuits, 1000, CalibrationMode::Disabled)?;
    /// ```
    pub fn run_with_mode(
        &self,
        circuits: Vec<CircuitInput>,
        shots: usize,
        calibration_mode: CalibrationMode,
    ) -> Result<TaskHandle, TianyanError> {
        TaskHandle::submit(
            self.client.clone(),
            circuits,
            shots,
            &self.name,
            calibration_mode,
        )
    }

    /// Like [`run`](Self::run) but always returns **raw** (uncalibrated) counts.
    pub fn run_raw(
        &self,
        circuits: Vec<CircuitInput>,
        shots: usize,
    ) -> Result<TaskHandle, TianyanError> {
        self.run_with_mode(circuits, shots, CalibrationMode::Disabled)
    }
}

/// Fetch the device list and return backends whose codes start with `tianyan`.
pub fn list_backends(client: Arc<TianyanClient>) -> Result<Vec<TianyanBackend>, TianyanError> {
    let resp: crate::client::ApiResponse<Vec<RawDevice>> = client.get(DEVICE_LIST_PATH)?;
    let raw_list = resp.into_data()?;
    Ok(raw_list
        .into_iter()
        .filter_map(|r| TianyanBackend::from_raw(r, client.clone()))
        .collect())
}

#[cfg(test)]
#[path = "device_test.rs"]
mod tests;
