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

//! Python bindings for the Tianyan quantum cloud platform client.
//!
//! Built with [PyO3](https://pyo3.rs/) and [maturin](https://maturin.rs/).
//!
//! # Module layout
//!
//! | Python name | Rust source |
//! |-------------|-------------|
//! | `TianyanPlatform` | `platform.rs` |
//! | `TianyanBackend` | `backend.rs` |
//! | `TaskHandle` | `task.rs` |
//! | `TianyanConfig` | `config.rs` |
//! | `DeviceStatus` | `backend.rs` |
//! | `DeviceType` | `backend.rs` |
//! | `DeviceToll` | `backend.rs` |
//! | `CalibrationMode` | `backend.rs` |
//! | `TianyanError` | `error.rs` |

pub mod backend;
pub mod config;
pub mod error;
pub mod platform;
pub mod task;

use backend::{PyCalibrationMode, PyDeviceStatus, PyDeviceToll, PyDeviceType, PyTianyanBackend};
use config::PyTianyanConfig;
use error::PyTianyanError;
use platform::PyTianyanPlatform;
use pyo3::prelude::*;
use task::PyTaskHandle;

#[pymodule]
#[pyo3(name = "_cqlib_tianyan")]
fn binding_python(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Exception type — must be registered before other classes so it can be
    // referenced in `#[pymethods]` error conversions at import time.
    m.add("TianyanError", py.get_type::<PyTianyanError>())?;

    m.add_class::<PyTianyanPlatform>()?;
    m.add_class::<PyTianyanBackend>()?;
    m.add_class::<PyTaskHandle>()?;
    m.add_class::<PyTianyanConfig>()?;
    m.add_class::<PyDeviceStatus>()?;
    m.add_class::<PyDeviceType>()?;
    m.add_class::<PyDeviceToll>()?;
    m.add_class::<PyCalibrationMode>()?;

    Ok(())
}
