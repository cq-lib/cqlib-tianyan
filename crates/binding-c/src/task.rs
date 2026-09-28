// This code is part of Cqlib.
//
// (C) Copyright China Telecom Quantum Group 2026
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.

//! C API for [`TaskHandle`] and `TianyanResultList`.
//!
//! # Workflow
//!
//! ```c
//! TianyanTask* task = tianyan_backend_run(backend, circuits, n, 1000);
//!
//! // Block until all circuits complete (120 s timeout, 5 s poll interval):
//! TianyanResultList* results = tianyan_task_wait(task, 120.0, 5.0);
//! if (!results) { fprintf(stderr, "%s\n", tianyan_last_error()); }
//!
//! size_t n_results = tianyan_result_list_len(results);
//! for (size_t i = 0; i < n_results; i++) {
//!     printf("task_id=%s shots=%zu\n",
//!            tianyan_result_task_id(results, i),
//!            tianyan_result_shots(results, i));
//!     char* json = tianyan_result_counts_json(results, i);
//!     printf("counts=%s\n", json);
//!     tianyan_string_free(json);
//! }
//!
//! tianyan_result_list_free(results);
//! tianyan_task_free(task);
//! ```

use cqlib_core::device::result::ExecutionResult;
use cqlib_core::device::result::Outcome;
use cqlib_tianyan::task::TaskHandle;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::time::Duration;

use crate::error::{clear_last_error, set_last_error, str_to_cstring};

/// Opaque wrapper around [`TaskHandle`] for C callers.
pub struct TianyanTaskC(pub(crate) TaskHandle);

/// A list of [`ExecutionResult`] returned by `tianyan_task_wait()` / `tianyan_task_status_snapshot()`.
///
/// `task_ids` is pre-built as a parallel `Vec<CString>` so that
/// `tianyan_result_task_id()` can return a stable `*const c_char` without
/// requiring the caller to free each individual string.
pub struct TianyanResultList {
    results: Vec<ExecutionResult>,
    /// Pre-built null-terminated task ID strings (one per result).
    task_ids: Vec<CString>,
}

impl TianyanResultList {
    fn new(results: Vec<ExecutionResult>) -> Self {
        let task_ids = results
            .iter()
            .map(|r| CString::new(r.task_id()).unwrap_or_default())
            .collect();
        Self { results, task_ids }
    }
}

// ── TaskHandle getters ────────────────────────────────────────────────────────

/// Return the name of the backend device this task was submitted to.
///
/// The returned pointer is valid for the lifetime of `task` and **must not**
/// be freed by the caller.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_device_name(task: *const TianyanTaskC) -> *const c_char {
    if task.is_null() {
        return std::ptr::null();
    }
    let t = unsafe { &*task };
    t.0.device_name.as_ptr() as *const c_char
}

/// Return the number of measurement shots requested per circuit.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_shots(task: *const TianyanTaskC) -> usize {
    if task.is_null() {
        return 0;
    }
    let t = unsafe { &*task };
    t.0.shots
}

/// Return the number of submitted circuits (= number of query IDs).
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_num_circuits(task: *const TianyanTaskC) -> usize {
    if task.is_null() {
        return 0;
    }
    let t = unsafe { &*task };
    t.0.task_ids.len()
}

/// Return a heap-allocated array of null-terminated query-ID strings.
///
/// Writes the count to `*out_len`. Each string and the array itself must be
/// freed by calling `tianyan_task_ids_free(ids, len)`.
///
/// Returns `NULL` on error.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_ids(
    task: *const TianyanTaskC,
    out_len: *mut usize,
) -> *mut *mut c_char {
    clear_last_error();
    if task.is_null() {
        set_last_error("task must not be NULL");
        return std::ptr::null_mut();
    }
    if out_len.is_null() {
        set_last_error("out_len must not be NULL");
        return std::ptr::null_mut();
    }
    let t = unsafe { &*task };
    let mut ptrs: Vec<*mut c_char> = t.0.task_ids.iter().map(|id| str_to_cstring(id)).collect();
    unsafe { *out_len = ptrs.len() };
    let raw = ptrs.as_mut_ptr();
    std::mem::forget(ptrs);
    raw
}

/// Free the array returned by `tianyan_task_ids()`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_ids_free(ids: *mut *mut c_char, len: usize) {
    if ids.is_null() || len == 0 {
        return;
    }
    unsafe {
        for i in 0..len {
            let s = *ids.add(i);
            if !s.is_null() {
                drop(std::ffi::CString::from_raw(s));
            }
        }
        drop(Vec::from_raw_parts(ids, len, len));
    }
}

/// Free a `TianyanTask` returned by any `tianyan_backend_run*()` or `tianyan_platform_submit()`.
///
/// Passing `NULL` is safe and does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_free(task: *mut TianyanTaskC) {
    if !task.is_null() {
        unsafe { drop(Box::from_raw(task)) };
    }
}

// ── Result polling ────────────────────────────────────────────────────────────

/// Block until **all** circuits have results, then return the list.
///
/// Readout error mitigation is applied according to the calibration mode set
/// when the task was submitted (default: `Auto`).
///
/// - `timeout_secs`: maximum wall-clock seconds to wait.
/// - `poll_interval_secs`: seconds between consecutive poll requests.
///
/// Returns a heap-allocated `TianyanResultList` on success, `NULL` on error.
/// The caller must free it with `tianyan_result_list_free()`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_wait(
    task: *const TianyanTaskC,
    timeout_secs: f64,
    poll_interval_secs: f64,
) -> *mut TianyanResultList {
    clear_last_error();
    if task.is_null() {
        set_last_error("task must not be NULL");
        return std::ptr::null_mut();
    }
    let t = unsafe { &*task };
    let timeout = Duration::from_secs_f64(timeout_secs);
    let interval = Duration::from_secs_f64(poll_interval_secs);
    match t.0.wait(timeout, interval) {
        Ok(results) => Box::into_raw(Box::new(TianyanResultList::new(results))),
        Err(e) => {
            set_last_error(e);
            std::ptr::null_mut()
        }
    }
}

/// Like `tianyan_task_wait()` but always returns raw (uncalibrated) counts,
/// regardless of the calibration mode set at submission time.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_wait_raw(
    task: *const TianyanTaskC,
    timeout_secs: f64,
    poll_interval_secs: f64,
) -> *mut TianyanResultList {
    clear_last_error();
    if task.is_null() {
        set_last_error("task must not be NULL");
        return std::ptr::null_mut();
    }
    let t = unsafe { &*task };
    let timeout = Duration::from_secs_f64(timeout_secs);
    let interval = Duration::from_secs_f64(poll_interval_secs);
    match t.0.wait_raw(timeout, interval) {
        Ok(results) => Box::into_raw(Box::new(TianyanResultList::new(results))),
        Err(e) => {
            set_last_error(e);
            std::ptr::null_mut()
        }
    }
}

/// Query the platform once and return whichever results are already ready.
///
/// Circuits that have not yet completed are absent from the returned list.
/// For polling until all circuits complete, use `tianyan_task_wait()`.
///
/// Returns a heap-allocated `TianyanResultList` on success, `NULL` on error.
/// The caller must free it with `tianyan_result_list_free()`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_task_status_snapshot(
    task: *const TianyanTaskC,
) -> *mut TianyanResultList {
    clear_last_error();
    if task.is_null() {
        set_last_error("task must not be NULL");
        return std::ptr::null_mut();
    }
    let t = unsafe { &*task };
    match t.0.status() {
        Ok(results) => Box::into_raw(Box::new(TianyanResultList::new(results))),
        Err(e) => {
            set_last_error(e);
            std::ptr::null_mut()
        }
    }
}

// ── TianyanResultList accessors ───────────────────────────────────────────────

/// Return the number of results in the list.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_result_list_len(list: *const TianyanResultList) -> usize {
    if list.is_null() {
        return 0;
    }
    let l = unsafe { &*list };
    l.results.len()
}

/// Return the task/query ID for the result at index `i`.
///
/// The returned pointer is valid for the lifetime of `list` and **must not**
/// be freed by the caller. Returns `NULL` if `i` is out of bounds.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_result_task_id(
    list: *const TianyanResultList,
    i: usize,
) -> *const c_char {
    if list.is_null() {
        return std::ptr::null();
    }
    let l = unsafe { &*list };
    match l.task_ids.get(i) {
        Some(cs) => cs.as_ptr(),
        None => std::ptr::null(),
    }
}

/// Return the number of shots for the result at index `i`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_result_shots(list: *const TianyanResultList, i: usize) -> usize {
    if list.is_null() {
        return 0;
    }
    let l = unsafe { &*list };
    l.results
        .get(i)
        .map(|r: &ExecutionResult| r.shots())
        .unwrap_or(0)
}

/// Return the number of measured qubits for the result at index `i`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_result_num_qubits(list: *const TianyanResultList, i: usize) -> usize {
    if list.is_null() {
        return 0;
    }
    let l = unsafe { &*list };
    l.results
        .get(i)
        .map(|r: &ExecutionResult| r.num_qubits())
        .unwrap_or(0)
}

/// Return the measurement counts for result `i` as a JSON object string.
///
/// Example: `{"00": 512, "11": 488}`
///
/// The returned string is heap-allocated and **must** be freed by the caller
/// with `tianyan_string_free()`. Returns `NULL` on error or out-of-bounds `i`.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_result_counts_json(
    list: *const TianyanResultList,
    i: usize,
) -> *mut c_char {
    if list.is_null() {
        return std::ptr::null_mut();
    }
    let l = unsafe { &*list };
    let result: &ExecutionResult = match l.results.get(i) {
        Some(r) => r,
        None => return std::ptr::null_mut(),
    };
    let num_qubits = result.num_qubits();
    let counts_map: HashMap<String, usize> = result
        .counts()
        .iter()
        .map(|(outcome, &count): (&Outcome, &usize)| (outcome.to_bitstring(num_qubits), count))
        .collect();
    match serde_json::to_string(&counts_map) {
        Ok(json) => str_to_cstring(&json),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a `TianyanResultList` returned by any `tianyan_task_wait*()` or
/// `tianyan_task_status_snapshot()` function.
///
/// Passing `NULL` is safe and does nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_result_list_free(list: *mut TianyanResultList) {
    if !list.is_null() {
        unsafe { drop(Box::from_raw(list)) };
    }
}
