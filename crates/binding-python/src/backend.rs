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
// Modified to expose the upgrading backend status.
// Modified to configure calibration through run and deprecate submission aliases.

//! Python bindings for [`TianyanBackend`], [`DeviceStatus`], [`DeviceToll`],
//! and [`CalibrationMode`].
//!
//! # Example
//!
//! ```python
//! from cqlib_tianyan import TianyanPlatform, DeviceStatus, CalibrationMode
//!
//! platform = TianyanPlatform.login("your_api_key")
//! backends = platform.list_backends()
//!
//! for b in backends:
//!     print(f"{b.name} ({b.status})")
//!
//! backend = platform.get_backend("tianyan-287")
//! print(f"{backend.num_qubits()} qubits")
//! if backend.is_available():
//!     task = backend.run(["H Q1\nM Q1"], shots=1000)
//!     # Or submit with calibration disabled:
//!     task2 = backend.run(["H Q1\nM Q1"], shots=1000, calibration_mode="disabled")
//! ```

use crate::error::IntoPyResult;
use crate::task::PyTaskHandle;
use cqlib_core::circuit::Instruction;
use cqlib_core::device::Device;
use cqlib_tianyan::device::{DeviceStatus, DeviceToll, DeviceType, TianyanBackend};
use cqlib_tianyan::task::CalibrationMode;
use pyo3::prelude::*;
use rustworkx_core::petgraph::visit::{EdgeRef, IntoEdgeReferences};

// ── CalibrationModeInput — accepts str or CalibrationMode from Python ─────────

/// A transparent wrapper that [`FromPyObject`] accepts from either a Python
/// `str` (`"auto"`, `"enabled"`, `"disabled"`) or a `CalibrationMode` object.
/// This makes function parameters polymorphic without requiring overloads.
pub(crate) struct CalibrationModeInput(pub(crate) CalibrationMode);

impl<'a, 'py> FromPyObject<'a, 'py> for CalibrationModeInput {
    type Error = PyErr;

    fn extract(obj: Borrowed<'a, 'py, PyAny>) -> Result<Self, Self::Error> {
        // Accept a CalibrationMode object directly.
        if let Ok(m) = obj.extract::<PyCalibrationMode>() {
            return Ok(CalibrationModeInput(m.inner));
        }
        // Accept a plain string — e.g. "auto", "enabled", "disabled".
        if let Ok(s) = obj.extract::<String>() {
            return Ok(CalibrationModeInput(PyCalibrationMode::new(&s)?.inner));
        }
        Err(pyo3::exceptions::PyTypeError::new_err(
            "calibration mode must be CalibrationMode or str ('auto', 'enabled', 'disabled')",
        ))
    }
}

/// Operational status of a Tianyan quantum backend.
///
/// # Values
/// - `"running"` — Device is online and accepting jobs.
/// - `"calibration"` — Device is being calibrated; submissions may queue.
/// - `"under_maintenance"` — Temporarily unavailable for maintenance.
/// - `"offline"` — Device is offline.
/// - `"upgrading"` — Device is undergoing an upgrade.
/// - `"unknown"` — Unrecognised status code from the API.
#[pyclass(name = "DeviceStatus", module = "cqlib_tianyan", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyDeviceStatus {
    inner: DeviceStatus,
}

impl From<DeviceStatus> for PyDeviceStatus {
    fn from(inner: DeviceStatus) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyDeviceStatus {
    /// The status as a string.
    #[getter]
    fn value(&self) -> &'static str {
        match &self.inner {
            DeviceStatus::Running => "running",
            DeviceStatus::Calibration => "calibration",
            DeviceStatus::UnderMaintenance => "under_maintenance",
            DeviceStatus::OffLine => "offline",
            DeviceStatus::Upgrading => "upgrading",
            DeviceStatus::Unknown(_) => "unknown",
        }
    }

    fn __repr__(&self) -> String {
        format!("DeviceStatus('{}')", self.value())
    }

    fn __str__(&self) -> &'static str {
        self.value()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(other_status) = other.extract::<PyDeviceStatus>() {
            self.inner == other_status.inner
        } else if let Ok(s) = other.extract::<String>() {
            self.value() == s.as_str()
        } else {
            false
        }
    }
}

/// Backend technology classified locally from the machine code.
///
/// Values: `"superconducting"`, `"photonic"`, `"ion_trap"`, `"simulator"`.
#[pyclass(name = "DeviceType", module = "cqlib_tianyan", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyDeviceType {
    inner: DeviceType,
}

impl From<DeviceType> for PyDeviceType {
    fn from(inner: DeviceType) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyDeviceType {
    /// The device type as a string.
    #[getter]
    fn value(&self) -> &'static str {
        match self.inner {
            DeviceType::Superconducting => "superconducting",
            DeviceType::Photonic => "photonic",
            DeviceType::IonTrap => "ion_trap",
            DeviceType::Simulator => "simulator",
        }
    }

    fn __repr__(&self) -> String {
        format!("DeviceType('{}')", self.value())
    }

    fn __str__(&self) -> &'static str {
        self.value()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(other_type) = other.extract::<PyDeviceType>() {
            self.inner == other_type.inner
        } else if let Ok(s) = other.extract::<String>() {
            self.value() == s.as_str()
        } else {
            false
        }
    }
}

/// Pricing model of a Tianyan quantum backend.
///
/// # Values
/// - `"free"` — No charge for submitting jobs.
/// - `"paid"` — Job submission consumes credits.
/// - `"unknown"` — Unrecognised pricing code from the API.
#[pyclass(name = "DeviceToll", module = "cqlib_tianyan", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyDeviceToll {
    inner: DeviceToll,
}

impl From<DeviceToll> for PyDeviceToll {
    fn from(inner: DeviceToll) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyDeviceToll {
    /// The pricing model as a string.
    #[getter]
    fn value(&self) -> &'static str {
        match &self.inner {
            DeviceToll::Free => "free",
            DeviceToll::Paid => "paid",
            DeviceToll::Unknown(_) => "unknown",
        }
    }

    fn __repr__(&self) -> String {
        format!("DeviceToll('{}')", self.value())
    }

    fn __str__(&self) -> &'static str {
        self.value()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(other_toll) = other.extract::<PyDeviceToll>() {
            self.inner == other_toll.inner
        } else if let Ok(s) = other.extract::<String>() {
            self.value() == s.as_str()
        } else {
            false
        }
    }
}

/// Controls readout error mitigation when fetching task results.
///
/// Pass as `calibration_mode` to `TianyanBackend.run()` to configure behaviour:
///
/// | Mode | Behaviour |
/// |------|-----------|
/// | `"auto"` | Apply mitigation on superconducting devices if data is available and at most 14 qubits are measured; otherwise return raw counts (default). |
/// | `"enabled"` | Require a superconducting device and calibration data; reject other device types before submission. |
/// | `"disabled"` | Never apply mitigation; always return raw counts. |
#[pyclass(name = "CalibrationMode", module = "cqlib_tianyan", from_py_object)]
#[derive(Clone, Debug)]
pub struct PyCalibrationMode {
    pub(crate) inner: CalibrationMode,
}

impl From<CalibrationMode> for PyCalibrationMode {
    fn from(inner: CalibrationMode) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyCalibrationMode {
    /// Create a CalibrationMode from a string value.
    ///
    /// # Arguments
    /// * `value` - One of `"auto"`, `"enabled"`, `"disabled"`.
    ///
    /// # Raises
    /// `ValueError` if the string is not recognised.
    #[new]
    fn new(value: &str) -> PyResult<Self> {
        let inner = match value {
            "auto" => CalibrationMode::Auto,
            "enabled" => CalibrationMode::Enabled,
            "disabled" => CalibrationMode::Disabled,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Unknown CalibrationMode '{}'; expected 'auto', 'enabled', or 'disabled'",
                    other
                )));
            }
        };
        Ok(Self { inner })
    }

    #[getter]
    fn value(&self) -> &'static str {
        match self.inner {
            CalibrationMode::Auto => "auto",
            CalibrationMode::Enabled => "enabled",
            CalibrationMode::Disabled => "disabled",
        }
    }

    fn __repr__(&self) -> String {
        format!("CalibrationMode('{}')", self.value())
    }

    fn __str__(&self) -> &'static str {
        self.value()
    }

    /// Compare with another `CalibrationMode` or with a plain string.
    ///
    /// This allows the natural Python idiom:
    ///
    /// ```python
    /// mode = CalibrationMode("auto")
    /// assert mode == "auto"
    /// assert mode == CalibrationMode("auto")
    /// ```
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        if let Ok(other_mode) = other.extract::<PyCalibrationMode>() {
            self.inner == other_mode.inner
        } else if let Ok(s) = other.extract::<String>() {
            self.value() == s.as_str()
        } else {
            false
        }
    }

    fn __hash__(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        self.value().hash(&mut h);
        h.finish()
    }
}

/// A quantum computing backend available on the Tianyan cloud platform.
///
/// Obtained via `TianyanPlatform.list_backends()` or `TianyanPlatform.get_backend()`.
///
/// # Example
///
/// ```python
/// backend = platform.get_backend("tianyan-287")
/// if backend.is_available():
///     task = backend.run(["H Q1\nM Q1"], shots=1000)
///     results = task.wait(timeout=120.0)
///     print(results[0].counts)
/// ```
#[pyclass(name = "TianyanBackend", module = "cqlib_tianyan")]
pub struct PyTianyanBackend {
    pub(crate) inner: TianyanBackend,
}

impl From<TianyanBackend> for PyTianyanBackend {
    fn from(inner: TianyanBackend) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyTianyanBackend {
    /// Machine code used as the backend identifier in submissions.
    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    /// User-friendly display name.
    #[getter]
    fn display_name(&self) -> &str {
        &self.inner.display_name
    }

    /// Backend technology classified locally from the machine code.
    #[getter]
    fn device_type(&self) -> PyDeviceType {
        self.inner.device_type.into()
    }

    /// Current operational status.
    #[getter]
    fn status(&self) -> PyDeviceStatus {
        self.inner.status.clone().into()
    }

    /// Pricing model.
    #[getter]
    fn toll(&self) -> PyDeviceToll {
        self.inner.toll.clone().into()
    }

    /// Returns `True` when the backend is in `"running"` status.
    fn is_available(&self) -> bool {
        self.inner.is_available()
    }

    /// Return the total number of physical qubits in the backend configuration.
    ///
    /// Downloads the backend configuration on first use and reuses the cached
    /// configuration afterwards. Disabled qubits are included in this count.
    /// Only superconducting backends support this operation.
    fn num_qubits(&self, py: Python<'_>) -> PyResult<usize> {
        self.inner.num_qubits().map_py_err(py)
    }

    /// Submit circuits and return a task handle.
    ///
    /// Only superconducting devices and simulators can submit tasks. Readout error
    /// mitigation is applied automatically on superconducting devices when data is
    /// available and at most 14 qubits are measured (`"auto"` is the default).
    ///
    /// # Arguments
    /// * `circuits` - List of QCIS circuit strings.
    /// * `shots` - Number of measurement shots per circuit.
    /// * `calibration_mode` - Keyword-only calibration policy: `"auto"` (default),
    ///   `"enabled"`, or `"disabled"`. Accepts a string or `CalibrationMode` object.
    ///   `"enabled"` rejects non-superconducting devices before submission;
    ///   missing calibration data raises an error when retrieving results.
    ///
    /// # Returns
    /// A `TaskHandle` that can be used to poll for results.
    #[pyo3(
        signature = (circuits, shots, *, calibration_mode = CalibrationModeInput(CalibrationMode::Auto)),
        text_signature = "($self, circuits, shots, *, calibration_mode='auto')"
    )]
    fn run(
        &self,
        py: Python<'_>,
        circuits: Vec<String>,
        shots: usize,
        calibration_mode: CalibrationModeInput,
    ) -> PyResult<PyTaskHandle> {
        let circuit_inputs: Vec<cqlib_tianyan::device::CircuitInput> =
            circuits.into_iter().map(|s| s.into()).collect();
        self.inner
            .run_with_mode(circuit_inputs, shots, calibration_mode.0)
            .map_py_err(py)
            .map(PyTaskHandle::from)
    }

    /// Deprecated alias for `run(..., calibration_mode="disabled")`.
    fn run_raw(
        &self,
        py: Python<'_>,
        circuits: Vec<String>,
        shots: usize,
    ) -> PyResult<PyTaskHandle> {
        PyErr::warn(
            py,
            &py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
            c"run_raw() is deprecated; use run(..., calibration_mode='disabled')",
            1,
        )?;
        self.run(
            py,
            circuits,
            shots,
            CalibrationModeInput(CalibrationMode::Disabled),
        )
    }

    /// Deprecated alias for `run(..., calibration_mode=mode)`.
    /// `"enabled"` requires a superconducting backend; other types fail before submission.
    ///
    /// # Arguments
    /// * `circuits` - List of QCIS circuit strings.
    /// * `shots` - Number of measurement shots per circuit.
    /// * `mode` - Calibration mode: `"auto"` (default), `"enabled"`, or `"disabled"`.
    ///   Accepts either a plain string or a `CalibrationMode` object.
    #[pyo3(signature = (circuits, shots, mode=None))]
    fn run_with_mode(
        &self,
        py: Python<'_>,
        circuits: Vec<String>,
        shots: usize,
        mode: Option<CalibrationModeInput>,
    ) -> PyResult<PyTaskHandle> {
        PyErr::warn(
            py,
            &py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
            c"run_with_mode() is deprecated; use run(..., calibration_mode=mode)",
            1,
        )?;
        self.run(
            py,
            circuits,
            shots,
            mode.unwrap_or(CalibrationModeInput(CalibrationMode::Auto)),
        )
    }

    /// Download the device calibration configuration (superconducting backends only).
    ///
    /// Returns a `cqlib.device.Device` object populated with topology,
    /// qubit properties, gate errors, and readout fidelities.
    ///
    /// The result is cached after the first call.
    fn device_config(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.inner
            .with_device(|device| device_to_py(py, device))
            .map_py_err(py)?
    }

    fn __repr__(&self) -> String {
        format!(
            "TianyanBackend(name='{}', device_type='{}', status='{}')",
            self.inner.name,
            PyDeviceType::from(self.inner.device_type).value(),
            PyDeviceStatus::from(self.inner.status.clone()).value(),
        )
    }
}

/// Convert a [`cqlib_core::device::Device`] into a Python `cqlib.device.Device` object.
///
/// This calls back into the installed `cqlib` Python package so users receive
/// a native `cqlib.device.Device` instance, fully compatible with the rest of the
/// `cqlib` ecosystem.
pub(crate) fn device_to_py(py: Python<'_>, device: &Device) -> PyResult<Py<PyAny>> {
    let cqlib_device = py.import("cqlib.device")?;
    let cqlib_circuit = py.import("cqlib.circuit")?;
    let device_class = cqlib_device.getattr("Device")?;
    let topology_class = cqlib_device.getattr("Topology")?;
    let instruction_class = cqlib_circuit.getattr("Instruction")?;
    let standard_gate_class = cqlib_circuit.getattr("StandardGate")?;

    // Build qubit index list
    let qubit_indices: Vec<u32> = device.qubits().map(|q| q.id()).collect();

    // Build topology edge list: [(control_idx, target_idx, gate_name), ...]
    // Need to map from graph NodeIndex to actual Qubit index
    let topology_inner = device.topology();
    let graph = topology_inner.graph();

    // Create a mapping from NodeIndex to Qubit index
    let node_to_qubit: std::collections::HashMap<_, _> = graph
        .node_indices()
        .map(|node_idx| {
            let qubit = graph[node_idx];
            (node_idx, qubit.id())
        })
        .collect();

    let edges: Vec<(u32, u32, String)> = graph
        .edge_references()
        .map(|e| {
            let source_idx = *node_to_qubit.get(&e.source()).ok_or_else(|| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "Internal error: node {:?} not in qubit map",
                    e.source()
                ))
            })?;
            let target_idx = *node_to_qubit.get(&e.target()).ok_or_else(|| {
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "Internal error: node {:?} not in qubit map",
                    e.target()
                ))
            })?;
            Ok((source_idx, target_idx, e.weight().clone()))
        })
        .collect::<PyResult<_>>()?;

    let py_topology = topology_class.call1((qubit_indices.clone(), edges))?;
    let py_device = device_class.call1((device.name(), qubit_indices, py_topology))?;

    let native_gates = device
        .native_gates()
        .iter()
        .map(|instruction| match instruction {
            Instruction::Standard(gate) => {
                let py_gate = standard_gate_class.getattr(gate.to_string())?;
                instruction_class.call_method1("from_standard_gate", (py_gate,))
            }
            other => Err(pyo3::exceptions::PyValueError::new_err(format!(
                "cannot convert non-standard native gate to Python: {other}"
            ))),
        })
        .collect::<PyResult<Vec<_>>>()?;
    py_device.setattr("native_gates", native_gates)?;

    Ok(py_device.into())
}
