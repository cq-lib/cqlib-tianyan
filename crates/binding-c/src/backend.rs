// This code is part of Cqlib.
//
// (C) Copyright China Telecom Quantum Group 2026
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.

//! C API for [`TianyanBackend`] and the list returned by `tianyan_platform_list_backends()`.
//!
//! # Memory ownership
//!
//! Individual `TianyanBackend` pointers must be freed with `tianyan_backend_free()`.
//! Arrays returned by `tianyan_platform_list_backends()` must be freed with
//! `tianyan_backend_list_free(array, len)` — this frees each element and the array itself.
//!
//! # CalibrationMode integer codes
//!
//! | Code | Mode     |
//! |------|----------|
//! | 0    | Auto     |
//! | 1    | Enabled  |
//! | 2    | Disabled |

use cqlib_tianyan::device::{DeviceStatus, DeviceToll, DeviceType, TianyanBackend};
use cqlib_tianyan::task::CalibrationMode;
use std::os::raw::{c_char, c_int};

use crate::error::{clear_last_error, set_last_error};
use crate::platform::collect_circuits;
use crate::task::TianyanTaskC;

/// Opaque wrapper around [`TianyanBackend`] for C callers.
pub struct TianyanBackendC(pub(crate) TianyanBackend);

/// Return the machine-code identifier of the backend (e.g. `"tianyan-287"`).
///
/// The returned pointer is valid for the lifetime of `backend` and **must not**
/// be freed by the caller.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_name(backend: *const TianyanBackendC) -> *const c_char {
    if backend.is_null() {
        return std::ptr::null();
    }
    // SAFETY: created by this crate; name is a Rust String stored inside the struct.
    let b = unsafe { &*backend };
    // Return a pointer into the Rust String — valid as long as `backend` is alive.
    b.0.name.as_ptr() as *const c_char
}

/// Return the user-friendly display name of the backend.
///
/// The returned pointer is valid for the lifetime of `backend` and **must not**
/// be freed by the caller.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_display_name(backend: *const TianyanBackendC) -> *const c_char {
    if backend.is_null() {
        return std::ptr::null();
    }
    let b = unsafe { &*backend };
    b.0.display_name.as_ptr() as *const c_char
}

/// Return the backend technology, classified locally from its machine code.
///
/// | Code | Device type     |
/// |------|-----------------|
/// | 0    | Superconducting |
/// | 1    | Photonic        |
/// | 2    | IonTrap         |
/// | 3    | Simulator       |
/// | -1   | NULL backend    |
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_device_type(backend: *const TianyanBackendC) -> c_int {
    if backend.is_null() {
        return -1;
    }
    let b = unsafe { &*backend };
    match b.0.device_type {
        DeviceType::Superconducting => 0,
        DeviceType::Photonic => 1,
        DeviceType::IonTrap => 2,
        DeviceType::Simulator => 3,
    }
}

/// Return the operational status of the backend as an integer code.
///
/// | Code | Status              |
/// |------|---------------------|
/// | 0    | Running             |
/// | 1    | Calibration         |
/// | 2    | UnderMaintenance    |
/// | 3    | OffLine             |
/// | 4    | Upgrading           |
/// | -1   | Unknown             |
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_status(backend: *const TianyanBackendC) -> c_int {
    if backend.is_null() {
        return -1;
    }
    let b = unsafe { &*backend };
    match b.0.status {
        DeviceStatus::Running => 0,
        DeviceStatus::Calibration => 1,
        DeviceStatus::UnderMaintenance => 2,
        DeviceStatus::OffLine => 3,
        DeviceStatus::Upgrading => 4,
        DeviceStatus::Unknown(_) => -1,
    }
}

/// Return the pricing model of the backend as an integer code.
///
/// | Code | Pricing |
/// |------|---------|
/// | 1    | Free    |
/// | 2    | Paid    |
/// | -1   | Unknown |
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_toll(backend: *const TianyanBackendC) -> c_int {
    if backend.is_null() {
        return -1;
    }
    let b = unsafe { &*backend };
    match b.0.toll {
        DeviceToll::Free => 1,
        DeviceToll::Paid => 2,
        DeviceToll::Unknown(_) => -1,
    }
}

/// Return `true` if the backend is in `Running` status and accepting jobs.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_is_available(backend: *const TianyanBackendC) -> bool {
    if backend.is_null() {
        return false;
    }
    let b = unsafe { &*backend };
    b.0.is_available()
}

/// Write the total number of physical qubits in the backend configuration to `out_num_qubits`.
///
/// This may download the backend configuration on first use and reuses the cached
/// configuration afterwards. Disabled qubits are included in this count.
/// Only superconducting backends support this operation; other types set an error.
///
/// Returns `true` on success and `false` on error. On error, call
/// `tianyan_last_error()` for details.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_num_qubits(
    backend: *const TianyanBackendC,
    out_num_qubits: *mut usize,
) -> bool {
    clear_last_error();
    if backend.is_null() {
        set_last_error("backend must not be NULL");
        return false;
    }
    if out_num_qubits.is_null() {
        set_last_error("out_num_qubits must not be NULL");
        return false;
    }

    let b = unsafe { &*backend };
    match b.0.num_qubits() {
        Ok(num_qubits) => {
            unsafe {
                *out_num_qubits = num_qubits;
            }
            true
        }
        Err(e) => {
            set_last_error(e);
            false
        }
    }
}

/// Free a `TianyanBackend` returned by any API function.
///
/// Passing `NULL` is safe and does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_free(backend: *mut TianyanBackendC) {
    if !backend.is_null() {
        unsafe { drop(Box::from_raw(backend)) };
    }
}

/// Free an array of `TianyanBackend` pointers returned by `tianyan_platform_list_backends()`.
///
/// This frees each individual backend and the array itself.
/// Passing `NULL` for `backends` is safe and does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_list_free(backends: *mut *mut TianyanBackendC, len: usize) {
    if backends.is_null() || len == 0 {
        return;
    }
    // SAFETY: `backends` is the array returned by `tianyan_platform_list_backends`.
    unsafe {
        for i in 0..len {
            let ptr = *backends.add(i);
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
        }
        // Reconstruct the Vec to free the array allocation.
        drop(Vec::from_raw_parts(backends, len, len));
    }
}

fn parse_calibration_mode(mode: c_int) -> Result<CalibrationMode, String> {
    match mode {
        0 => Ok(CalibrationMode::Auto),
        1 => Ok(CalibrationMode::Enabled),
        2 => Ok(CalibrationMode::Disabled),
        other => Err(format!(
            "Unknown CalibrationMode code {other}; expected 0=Auto, 1=Enabled, 2=Disabled"
        )),
    }
}

/// Submit circuits on this backend with the default calibration mode (`Auto`).
/// Only superconducting devices and simulators can submit tasks. Simulators return
/// raw counts without downloading configuration or applying readout calibration.
///
/// - `circuits`: array of `n_circuits` null-terminated QCIS strings.
/// - `shots`: number of measurement shots per circuit.
///
/// Returns a heap-allocated `TianyanTask` on success, `NULL` on error.
/// The caller must free the returned pointer with `tianyan_task_free()`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_run(
    backend: *const TianyanBackendC,
    circuits: *const *const c_char,
    n_circuits: usize,
    shots: usize,
) -> *mut TianyanTaskC {
    clear_last_error();
    if backend.is_null() {
        set_last_error("backend must not be NULL");
        return std::ptr::null_mut();
    }
    let circuit_inputs = match collect_circuits(circuits, n_circuits) {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return std::ptr::null_mut();
        }
    };
    let b = unsafe { &*backend };
    match b.0.run(circuit_inputs, shots) {
        Ok(t) => Box::into_raw(Box::new(TianyanTaskC(t))),
        Err(e) => {
            set_last_error(e);
            std::ptr::null_mut()
        }
    }
}

/// Like `tianyan_backend_run()` but always returns raw (uncalibrated) counts.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_run_raw(
    backend: *const TianyanBackendC,
    circuits: *const *const c_char,
    n_circuits: usize,
    shots: usize,
) -> *mut TianyanTaskC {
    clear_last_error();
    if backend.is_null() {
        set_last_error("backend must not be NULL");
        return std::ptr::null_mut();
    }
    let circuit_inputs = match collect_circuits(circuits, n_circuits) {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return std::ptr::null_mut();
        }
    };
    let b = unsafe { &*backend };
    match b.0.run_raw(circuit_inputs, shots) {
        Ok(t) => Box::into_raw(Box::new(TianyanTaskC(t))),
        Err(e) => {
            set_last_error(e);
            std::ptr::null_mut()
        }
    }
}

/// Like `tianyan_backend_run()` but with an explicit calibration mode.
///
/// `mode`: 0 = Auto, 1 = Enabled, 2 = Disabled.
/// Enabled requires a superconducting device; other types fail before submission.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_backend_run_with_mode(
    backend: *const TianyanBackendC,
    circuits: *const *const c_char,
    n_circuits: usize,
    shots: usize,
    mode: c_int,
) -> *mut TianyanTaskC {
    clear_last_error();
    if backend.is_null() {
        set_last_error("backend must not be NULL");
        return std::ptr::null_mut();
    }
    let cal_mode = match parse_calibration_mode(mode) {
        Ok(m) => m,
        Err(e) => {
            set_last_error(e);
            return std::ptr::null_mut();
        }
    };
    let circuit_inputs = match collect_circuits(circuits, n_circuits) {
        Ok(v) => v,
        Err(e) => {
            set_last_error(e);
            return std::ptr::null_mut();
        }
    };
    let b = unsafe { &*backend };
    match b.0.run_with_mode(circuit_inputs, shots, cal_mode) {
        Ok(t) => Box::into_raw(Box::new(TianyanTaskC(t))),
        Err(e) => {
            set_last_error(e);
            std::ptr::null_mut()
        }
    }
}
