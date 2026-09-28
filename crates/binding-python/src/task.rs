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

//! Python bindings for [`TaskHandle`] and result helpers.
//!
//! [`PyTaskHandle`] wraps a submitted batch of circuits.  Its `wait()` and
//! `status()` methods return **native `cqlib.device.ExecutionResult`** objects
//! by calling back into the installed `cqlib` Python package — no type duplication.
//!
//! # Example
//!
//! ```python
//! task = backend.run(["H Q1\nM Q1"], shots=1000)
//!
//! # Non-blocking snapshot
//! partial = task.status()
//!
//! # Block until all results are ready (releases GIL while polling)
//! results = task.wait(timeout=120.0, poll_interval=5.0)
//! for r in results:
//!     print(r.task_id, r.counts, r.probabilities)
//! ```

use crate::error::IntoPyResult;
use cqlib_core::device::result::ExecutionResult;
use cqlib_tianyan::task::TaskHandle;
use pyo3::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

/// Convert a [`cqlib_core::device::result::ExecutionResult`] into a Python
/// `cqlib.device.ExecutionResult` object by calling back into the installed
/// `cqlib` package.
///
/// This ensures callers receive a native `cqlib` type, fully compatible with
/// the rest of the `cqlib` ecosystem.
pub(crate) fn er_to_py(py: Python<'_>, er: &ExecutionResult) -> PyResult<Py<PyAny>> {
    let cqlib_device = py.import("cqlib.device")?;
    let er_class = cqlib_device.getattr("ExecutionResult")?;
    let qubit_class = py.import("cqlib")?.getattr("Qubit")?;

    // Build the list of cqlib.Qubit objects.
    let qubits: Vec<Py<PyAny>> = er
        .qubits()
        .iter()
        .map(|q| qubit_class.call1((q.index(),)).map(|o| o.into()))
        .collect::<PyResult<_>>()?;

    // Construct ExecutionResult(task_id, qubits, shots, num_qubits, backend)
    let py_er = er_class.call1((
        er.task_id(),
        qubits,
        er.shots(),
        er.num_qubits(),
        er.backend().cloned().into_pyobject(py)?,
    ))?;

    // Advance lifecycle: start → finish(counts) → calc_probabilities
    py_er.call_method0("start")?;

    let counts: HashMap<String, usize> = er
        .counts()
        .iter()
        .map(|(outcome, &count)| (outcome.to_string(er.num_qubits()), count))
        .collect();
    py_er.call_method1("finish", (counts,))?;
    py_er.call_method0("calc_probabilities")?;

    Ok(py_er.into())
}

/// Convert a `Vec<ExecutionResult>` into a Python list of `cqlib.device.ExecutionResult`.
fn results_to_py(py: Python<'_>, results: Vec<ExecutionResult>) -> PyResult<Vec<Py<PyAny>>> {
    results.iter().map(|er| er_to_py(py, er)).collect()
}

/// A batch of circuits submitted to the Tianyan quantum cloud platform.
///
/// Obtained from `TianyanBackend.run()` or `TianyanPlatform.submit()`.
/// Use `wait()` to block until all results are available, or `status()` for
/// a non-blocking snapshot of available results.
///
/// Results are returned as `cqlib.device.ExecutionResult` objects.
#[pyclass(name = "TaskHandle", module = "cqlib_tianyan")]
pub struct PyTaskHandle {
    inner: TaskHandle,
}

impl From<TaskHandle> for PyTaskHandle {
    fn from(inner: TaskHandle) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyTaskHandle {
    /// Platform-assigned query IDs for all submitted circuits.
    #[getter]
    fn task_ids(&self) -> Vec<String> {
        self.inner.task_ids.clone()
    }

    /// Name of the backend device used for the submission.
    #[getter]
    fn device_name(&self) -> &str {
        &self.inner.device_name
    }

    /// Number of shots requested per circuit.
    #[getter]
    fn shots(&self) -> usize {
        self.inner.shots
    }

    /// Submission timestamp in ISO 8601 format.
    #[getter]
    fn submitted_at(&self) -> String {
        self.inner.submitted_at.to_string()
    }

    /// Query the platform once and return whichever results are ready.
    ///
    /// Circuits that have not completed yet are absent from the returned list.
    /// For polling until all circuits complete, use `wait()` instead.
    ///
    /// # Returns
    /// List of `cqlib.device.ExecutionResult` for completed circuits.
    fn status(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        let results = py.detach(|| self.inner.status()).map_py_err(py)?;
        results_to_py(py, results)
    }

    /// Block until **all** submitted circuits have results, then return them.
    ///
    /// The GIL is released while waiting so other Python threads remain active.
    ///
    /// Readout error mitigation is applied according to the calibration mode set
    /// when the task was submitted (default: `"auto"`).
    ///
    /// # Arguments
    /// * `timeout` - Maximum wall-clock time to wait, in seconds (default: `120.0`).
    /// * `poll_interval` - Time between consecutive poll requests, in seconds (default: `5.0`).
    ///
    /// # Raises
    /// `TianyanError` if the timeout is exceeded before all results are available.
    #[pyo3(signature = (timeout = 120.0, poll_interval = 5.0))]
    fn wait(&self, py: Python<'_>, timeout: f64, poll_interval: f64) -> PyResult<Vec<Py<PyAny>>> {
        let timeout = Duration::from_secs_f64(timeout);
        let interval = Duration::from_secs_f64(poll_interval);
        let results = py
            .detach(|| self.inner.wait(timeout, interval))
            .map_py_err(py)?;
        results_to_py(py, results)
    }

    /// Like `wait()`, but always returns **raw** (uncalibrated) counts regardless
    /// of the calibration mode set at submission time.
    ///
    /// # Arguments
    /// * `timeout` - Maximum wall-clock time to wait, in seconds (default: `120.0`).
    /// * `poll_interval` - Time between consecutive poll requests, in seconds (default: `5.0`).
    #[pyo3(signature = (timeout = 120.0, poll_interval = 5.0))]
    fn wait_raw(
        &self,
        py: Python<'_>,
        timeout: f64,
        poll_interval: f64,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let timeout = Duration::from_secs_f64(timeout);
        let interval = Duration::from_secs_f64(poll_interval);
        let results = py
            .detach(|| self.inner.wait_raw(timeout, interval))
            .map_py_err(py)?;
        results_to_py(py, results)
    }

    fn __repr__(&self) -> String {
        format!(
            "TaskHandle(device='{}', shots={}, n_circuits={})",
            self.inner.device_name,
            self.inner.shots,
            self.inner.task_ids.len(),
        )
    }
}
