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

//! Task submission and result polling for the Tianyan quantum cloud platform.
//!
//! [`TaskHandle`] represents a batch of circuits submitted to the backend.  It holds
//! the returned `query_ids` and can poll the platform for results via
//! [`TaskHandle::wait`] (blocking poll-until-complete) or [`TaskHandle::status`]
//! (single snapshot query).
//!
//! # Batch splitting
//!
//! The Tianyan platform accepts at most [`MAX_BATCH_SIZE`] (50) circuits per submission
//! request.  [`TaskHandle::submit`] splits larger inputs automatically, submits them
//! sequentially, and merges the returned `query_ids` into a single `TaskHandle`.

use crate::calibration;
use crate::client::TianyanClient;
use crate::config::{QUERY_RESULT_PATH, SUBMIT_PATH};
use crate::device::{CircuitInput, DeviceType};
use crate::device_config::{self, ReadoutCalibrationData};
use crate::error::TianyanError;
use cqlib_core::circuit::Qubit;
use cqlib_core::device::result::{ExecutionResult, Outcome, Status};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use time::OffsetDateTime;

/// A map from qubit hardware index to `(f00, f11)` readout fidelity pair.
///
/// Built from [`ReadoutCalibrationData::to_cal_map`] and used internally to
/// look up fidelities only for the qubits that were actually measured in a
/// circuit, rather than passing the full device-wide arrays to the calibration
/// routine (which would cause exponential 2^n × 2^n memory allocation).
type CalibrationMap = HashMap<u32, (f64, f64)>;

/// Maximum number of circuits allowed in a single submission request.
pub const MAX_BATCH_SIZE: usize = 50;

/// Maximum number of measured qubits for which [`CalibrationMode::Auto`] will
/// automatically apply readout error mitigation.
///
/// Building the inverse confusion matrix requires O(4^n) memory where n is the
/// number of measured qubits.  Above this threshold the cost becomes
/// impractical for interactive use (> 1 GiB RAM), so `Auto` mode silently
/// falls back to returning raw counts instead.
///
/// Use [`CalibrationMode::Enabled`] to override this limit when you know the
/// circuit is small enough or you have sufficient memory.
pub const AUTO_CALIBRATION_MAX_QUBITS: usize = 14;

/// Controls whether readout error mitigation is applied when fetching results.
///
/// Pass to [`TianyanBackend::run_with_mode`](crate::device::TianyanBackend::run_with_mode)
/// to configure the default behaviour for a [`TaskHandle`].
///
/// | Variant | Behaviour |
/// |---------|-----------|
/// | `Auto` | On superconducting devices, apply mitigation when calibration data is available **and** the circuit measures ≤ [`AUTO_CALIBRATION_MAX_QUBITS`] (14) qubits; otherwise return raw counts. Other device types skip configuration download. (**default**) |
/// | `Enabled` | Require a superconducting device and calibration data, regardless of circuit size. Reject other device types before submission. *Caller is responsible for ensuring sufficient memory.* |
/// | `Disabled` | Never apply mitigation; always return raw measurement counts. |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CalibrationMode {
    /// Apply readout calibration on superconducting devices when available and
    /// circuit measures ≤ 14 qubits; otherwise return raw counts (default).
    #[default]
    Auto,
    /// Always apply readout calibration regardless of qubit count;
    /// return an error for non-superconducting devices or unavailable calibration data.
    Enabled,
    /// Never apply readout calibration; always return raw counts.
    Disabled,
}

impl CalibrationMode {
    fn should_calibrate(self, device_name: &str) -> Result<bool, TianyanError> {
        let device_type = DeviceType::from_code(device_name)
            .ok_or_else(|| TianyanError::DeviceNotFound(device_name.to_string()))?;
        match self {
            Self::Disabled => Ok(false),
            Self::Auto => Ok(device_type == DeviceType::Superconducting),
            Self::Enabled if device_type == DeviceType::Superconducting => Ok(true),
            Self::Enabled => Err(TianyanError::InvalidInput(format!(
                "readout calibration is only supported for superconducting devices: {device_name}"
            ))),
        }
    }
}

/// Request body for the batch circuit submission endpoint.
///
/// Field names and structure mirror the Python SDK's `submit_experiment` call:
/// `circuit` (list of QCIS strings), `language`, `computerCode`, etc.
#[derive(Serialize)]
struct SubmitRequest {
    /// List of QCIS circuit strings (max 50 per request).
    circuit: Vec<String>,
    /// Quantum language identifier. Always `"qcis"` for QCIS circuits.
    language: &'static str,
    name: String,
    shots: usize,
    #[serde(rename = "computerCode")]
    computer_code: String,
    is_verify: bool,
    /// Optional lab group ID (null → no lab grouping).
    #[serde(skip_serializing_if = "Option::is_none")]
    lab_id: Option<String>,
    /// Optional lab name.
    #[serde(skip_serializing_if = "Option::is_none")]
    lab_name: Option<String>,
}

#[derive(Deserialize)]
struct SubmitResponseData {
    query_ids: Vec<String>,
}

#[derive(Serialize)]
struct QueryRequest {
    query_ids: Vec<String>,
}

#[derive(Deserialize)]
struct QueryResponseData {
    #[serde(rename = "experimentResultModelList")]
    results: Vec<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct RawResult {
    /// Task ID — may be a string or integer depending on the endpoint.
    #[serde(
        rename = "experimentTaskId",
        alias = "query_id",
        alias = "queryId",
        default
    )]
    query_id: Option<serde_json::Value>,
    #[allow(dead_code)]
    status: Option<i64>,
    /// Raw per-shot measurement data. Always present on completion.
    ///
    /// Format:
    /// - `rows[0]`: qubit index header, e.g. `[1, 8]` → Q1, Q8.
    /// - `rows[j]` (j≥1): measurements for shot j in header order.
    #[serde(rename = "resultStatus", default)]
    result_status: Option<serde_json::Value>,
    /// Pre-aggregated probability map (double-encoded JSON string).
    /// Not always present for large qubit counts or high shot counts.
    #[serde(default)]
    probability: Option<serde_json::Value>,
}

impl RawResult {
    fn task_id_str(&self) -> String {
        match &self.query_id {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => "unknown".to_string(),
        }
    }
}

/// Parse the `resultStatus` field into (qubit list, exact counts).
///
/// # Outcome bit convention
/// Header position `i` → `qubits[i]` = bit `i` (value `2^i`).
/// Bitstring is **big-endian**: `measurements[n-1]` is leftmost (MSB),
/// `measurements[0]` is rightmost (LSB) — matching `Outcome::from_bitstring`.
fn parse_result_status(val: serde_json::Value) -> Option<(Vec<Qubit>, HashMap<Outcome, usize>)> {
    let rows = val.as_array()?;
    if rows.len() < 2 {
        return None;
    }

    // Row 0 = qubit index header, e.g. [1, 8] for Q1, Q8.
    let header: Vec<u32> = rows[0]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64().map(|x| x as u32))
        .collect();
    let n = header.len();
    if n == 0 {
        return None;
    }

    // qubits[i] = Qubit(header[i])  →  qubits[i] ↔ bit i in Outcome.
    let qubits: Vec<Qubit> = header.iter().map(|&idx| Qubit::new(idx)).collect();

    let mut counts: HashMap<Outcome, usize> = HashMap::new();
    for row in &rows[1..] {
        let measurements = match row.as_array() {
            Some(a) if a.len() == n => a,
            _ => continue,
        };
        // Big-endian bitstring: measurements[n-1] first (MSB), measurements[0] last (LSB).
        let bitstr: String = measurements
            .iter()
            .rev()
            .map(|m| {
                if m.as_u64().unwrap_or(0) == 1 {
                    '1'
                } else {
                    '0'
                }
            })
            .collect();
        if let Ok(outcome) = Outcome::from_bitstring(&bitstr) {
            *counts.entry(outcome).or_insert(0) += 1;
        }
    }

    if counts.is_empty() {
        return None;
    }
    Some((qubits, counts))
}

/// Parse the `probability` field (object or double-encoded JSON string) into
/// a `HashMap<String, f64>` for fallback use when `resultStatus` is absent.
fn parse_probability(v: serde_json::Value) -> Option<HashMap<String, f64>> {
    let obj = match v {
        serde_json::Value::Object(m) => m,
        serde_json::Value::String(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => return None,
        },
        _ => return None,
    };
    let mut map = HashMap::new();
    for (k, v) in obj {
        if let Some(f) = v.as_f64() {
            map.insert(k, f);
        }
    }
    Some(map)
}

/// A batch of circuits that have been submitted to the Tianyan cloud platform.
///
/// This struct stores the returned `query_ids` and a reference to the HTTP client
/// for subsequent result queries.
///
/// # Obtaining a `TaskHandle`
///
/// Use [`TianyanBackend::run`](crate::device::TianyanBackend::run) or
/// [`TianyanPlatform::submit`](crate::platform::TianyanPlatform::submit).
#[derive(Clone)]
pub struct TaskHandle {
    /// Platform-assigned query identifiers for each submitted circuit.
    pub task_ids: Vec<String>,
    /// Name of the backend device used for the submission.
    pub device_name: String,
    /// Number of shots requested for each circuit.
    pub shots: usize,
    /// UTC timestamp recorded just before submission.
    pub submitted_at: OffsetDateTime,
    /// Controls whether [`wait`](Self::wait) applies readout error mitigation.
    ///
    /// Defaults to [`CalibrationMode::Auto`] — calibration is applied when
    /// the device is superconducting and calibration data is available, raw counts otherwise.
    pub calibration_mode: CalibrationMode,
    pub(crate) client: Arc<TianyanClient>,
}

impl std::fmt::Debug for TaskHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle")
            .field("task_ids", &self.task_ids)
            .field("device_name", &self.device_name)
            .field("shots", &self.shots)
            .field("submitted_at", &self.submitted_at)
            .finish()
    }
}

impl TaskHandle {
    /// Submit `circuits` to `device_name` and return a [`TaskHandle`].
    ///
    /// Inputs larger than [`MAX_BATCH_SIZE`] are split into multiple sequential
    /// sub-requests; all returned `query_ids` are merged into a single `TaskHandle`.
    pub(crate) fn submit(
        client: Arc<TianyanClient>,
        circuits: Vec<CircuitInput>,
        shots: usize,
        device_name: &str,
        calibration_mode: CalibrationMode,
    ) -> Result<Self, TianyanError> {
        if circuits.is_empty() {
            return Err(TianyanError::InvalidInput(
                "circuit list must not be empty".to_string(),
            ));
        }
        if shots == 0 {
            return Err(TianyanError::InvalidInput(
                "shots must be greater than zero".to_string(),
            ));
        }

        // Validate capabilities before converting circuits or submitting any batch.
        calibration_mode.should_calibrate(device_name)?;
        if !matches!(
            DeviceType::from_code(device_name),
            Some(DeviceType::Superconducting | DeviceType::Simulator)
        ) {
            return Err(TianyanError::InvalidInput(format!(
                "task submission is only supported for superconducting devices and simulators: {device_name}"
            )));
        }

        // Convert all circuit inputs to QCIS strings up-front.
        let qcis_list: Vec<String> = circuits
            .into_iter()
            .map(|c| c.into_qcis())
            .collect::<Result<_, _>>()?;

        let submitted_at = time::OffsetDateTime::now_utc();

        // Split into batches of MAX_BATCH_SIZE and submit sequentially.
        let chunks: Vec<Vec<String>> = qcis_list
            .chunks(MAX_BATCH_SIZE)
            .map(|c| c.to_vec())
            .collect();

        let mut all_ids: Vec<String> = Vec::new();
        for chunk in chunks.into_iter() {
            let req = SubmitRequest {
                circuit: chunk,
                language: "qcis",
                // Empty string avoids duplicate-name rejection; the returned
                // task IDs are used as the canonical identifiers afterwards.
                name: String::new(),
                shots,
                computer_code: device_name.to_string(),
                is_verify: true,
                lab_id: None,
                lab_name: None,
            };
            let resp: crate::client::ApiResponse<SubmitResponseData> =
                client.post(SUBMIT_PATH, &req)?;
            let data = resp.into_data()?;
            all_ids.extend(data.query_ids);
        }

        Ok(TaskHandle {
            task_ids: all_ids,
            device_name: device_name.to_string(),
            shots,
            submitted_at,
            calibration_mode,
            client,
        })
    }

    /// Returns the platform query IDs for all submitted circuits.
    pub fn task_ids(&self) -> &[String] {
        &self.task_ids
    }

    /// Query the platform once and return whatever results are available.
    ///
    /// Results for circuits that have not yet completed will be absent from the
    /// returned `Vec`.
    pub fn status(&self) -> Result<Vec<ExecutionResult>, TianyanError> {
        query_results(&self.client, &self.task_ids, self.shots)
    }

    /// Poll the platform until **all** submitted circuits have results, or until
    /// `timeout` elapses.
    ///
    /// Readout error mitigation is applied according to [`self.calibration_mode`](Self::calibration_mode):
    /// - [`Auto`](CalibrationMode::Auto) *(default)* — calibrate superconducting devices if data is available **and** the
    ///   circuit measures ≤ [`AUTO_CALIBRATION_MAX_QUBITS`] (14) qubits; else return raw.
    /// - [`Enabled`](CalibrationMode::Enabled) — require a superconducting device and calibration data.
    /// - [`Disabled`](CalibrationMode::Disabled) — always return raw counts.
    ///
    /// # Arguments
    /// * `timeout` — maximum wall-clock duration to wait.
    /// * `poll_interval` — how long to sleep between consecutive poll requests.
    ///
    /// # Errors
    /// Returns [`TianyanError::Timeout`] if `timeout` is exceeded before all
    /// results are available.
    pub fn wait(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<Vec<ExecutionResult>, TianyanError> {
        match self.calibration_mode {
            CalibrationMode::Disabled => self.wait_raw(timeout, poll_interval),
            CalibrationMode::Auto | CalibrationMode::Enabled => {
                self.wait_calibrated(timeout, poll_interval)
            }
        }
    }

    /// Like [`wait`](Self::wait), but always returns **raw** (uncalibrated) counts
    /// regardless of [`calibration_mode`](Self::calibration_mode).
    pub fn wait_raw(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<Vec<ExecutionResult>, TianyanError> {
        let start = Instant::now();
        let expected = self.task_ids.len();

        loop {
            let results = query_results(&self.client, &self.task_ids, self.shots)?;
            if results.len() == expected {
                return Ok(results);
            }

            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Err(TianyanError::Timeout(timeout));
            }

            let remaining = timeout - elapsed;
            std::thread::sleep(poll_interval.min(remaining));
        }
    }

    /// Like [`status`](Self::status), but applies readout error mitigation to
    /// the measured probabilities before building execution results.
    ///
    /// Only the fidelities for qubits that were actually measured in each circuit
    /// are used, avoiding the O(4^n_device) memory explosion.
    /// Respects the device type and [`calibration_mode`](Self::calibration_mode),
    /// even when calibration data is supplied explicitly.
    pub fn status_with_calibration(
        &self,
        cal: &ReadoutCalibrationData,
    ) -> Result<Vec<ExecutionResult>, TianyanError> {
        if !self.calibration_mode.should_calibrate(&self.device_name)? {
            return self.status();
        }
        let cal_map = cal.to_cal_map();
        query_results_calibrated(
            &self.client,
            &self.task_ids,
            self.shots,
            Some(&cal_map),
            self.calibration_mode,
        )
    }

    /// Like [`wait`](Self::wait), using supplied calibration data. Respects the
    /// device type and [`calibration_mode`](Self::calibration_mode).
    pub fn wait_with_calibration(
        &self,
        timeout: Duration,
        poll_interval: Duration,
        cal: &ReadoutCalibrationData,
    ) -> Result<Vec<ExecutionResult>, TianyanError> {
        if !self.calibration_mode.should_calibrate(&self.device_name)? {
            return self.wait_raw(timeout, poll_interval);
        }
        let cal_map = cal.to_cal_map();
        let start = Instant::now();
        let expected = self.task_ids.len();

        loop {
            let results = query_results_calibrated(
                &self.client,
                &self.task_ids,
                self.shots,
                Some(&cal_map),
                self.calibration_mode,
            )?;
            if results.len() == expected {
                return Ok(results);
            }

            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Err(TianyanError::Timeout(timeout));
            }

            let remaining = timeout - elapsed;
            std::thread::sleep(poll_interval.min(remaining));
        }
    }

    /// Like [`wait`](Self::wait), but automatically downloads readout
    /// calibration data from the backend and applies measurement error
    /// mitigation to the results.
    ///
    /// Respects [`calibration_mode`](Self::calibration_mode): `Auto` skips download
    /// for non-superconducting devices and falls back to raw counts for missing
    /// calibration data. `Enabled` requires a superconducting device and calibration
    /// data. `Disabled` returns raw counts without downloading configuration.
    pub fn wait_calibrated(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<Vec<ExecutionResult>, TianyanError> {
        if !self.calibration_mode.should_calibrate(&self.device_name)? {
            return self.wait_raw(timeout, poll_interval);
        }
        let cal = device_config::download_device_config_full(&self.client, &self.device_name)?;
        match cal.1 {
            Some(ref data) => self.wait_with_calibration(timeout, poll_interval, data),
            None => match self.calibration_mode {
                CalibrationMode::Enabled => Err(TianyanError::InvalidInput(
                    "readout calibration data not available for this backend".to_string(),
                )),
                _ => self.wait_raw(timeout, poll_interval),
            },
        }
    }
}

/// Query the result endpoint for `task_ids` and parse raw responses into
/// [`ExecutionResult`] values from `cqlib-core`.
///
/// Prefers `resultStatus` (exact shot data) over the `probability` field.
/// Only circuits whose results are available are included in the return value.
fn query_results(
    client: &TianyanClient,
    task_ids: &[String],
    shots: usize,
) -> Result<Vec<ExecutionResult>, TianyanError> {
    let body = QueryRequest {
        query_ids: task_ids.to_vec(),
    };
    let resp: crate::client::ApiResponse<QueryResponseData> =
        client.post(QUERY_RESULT_PATH, &body)?;
    let data = resp.into_data()?;

    let mut results = Vec::new();
    for raw_val in data.results {
        let raw: RawResult = serde_json::from_value(raw_val).map_err(TianyanError::Json)?;
        let qid = raw.task_id_str();

        // Prefer resultStatus (exact counts); fall back to probability field.
        if let Some((qubits, counts)) = raw.result_status.and_then(parse_result_status) {
            if let Some(er) = build_er_from_counts(&qid, shots, qubits, counts, None) {
                results.push(er);
            }
        } else if let Some(prob_map) = raw.probability.and_then(parse_probability) {
            if let Some(er) = build_er_from_prob_map(&qid, shots, &prob_map) {
                results.push(er);
            }
        }
    }
    Ok(results)
}

/// Like [`query_results`] but optionally applies readout error mitigation.
///
/// The `calibration_mode` parameter controls the per-circuit threshold logic:
/// - [`CalibrationMode::Auto`] — skip calibration for circuits measuring more
///   than [`AUTO_CALIBRATION_MAX_QUBITS`] qubits (memory safety guard).
/// - [`CalibrationMode::Enabled`] — always calibrate regardless of qubit count.
/// - [`CalibrationMode::Disabled`] — never apply calibration.
fn query_results_calibrated(
    client: &TianyanClient,
    task_ids: &[String],
    shots: usize,
    cal: Option<&CalibrationMap>,
    calibration_mode: CalibrationMode,
) -> Result<Vec<ExecutionResult>, TianyanError> {
    let body = QueryRequest {
        query_ids: task_ids.to_vec(),
    };
    let resp: crate::client::ApiResponse<QueryResponseData> =
        client.post(QUERY_RESULT_PATH, &body)?;
    let data = resp.into_data()?;

    let mut results = Vec::new();
    for raw_val in data.results {
        let raw: RawResult = serde_json::from_value(raw_val).map_err(TianyanError::Json)?;
        let qid = raw.task_id_str();

        if let Some((qubits, counts)) = raw.result_status.and_then(parse_result_status) {
            // Auto mode: skip calibration if this circuit measures too many qubits.
            // Enabled mode: always calibrate (caller accepted the memory cost).
            let effective_cal = match calibration_mode {
                CalibrationMode::Disabled => None,
                CalibrationMode::Auto if qubits.len() > AUTO_CALIBRATION_MAX_QUBITS => None,
                _ => cal,
            };
            if let Some(er) = build_er_from_counts(&qid, shots, qubits, counts, effective_cal) {
                results.push(er);
            }
        } else if let Some(prob_map) = raw.probability.and_then(parse_probability) {
            // For the probability-only path, we don't have the measured qubit list,
            // so calibration cannot be applied per-qubit. Fall back to raw results.
            if let Some(er) = build_er_from_prob_map(&qid, shots, &prob_map) {
                results.push(er);
            }
        }
    }
    Ok(results)
}

/// Build an [`ExecutionResult`] from exact shot counts derived from `resultStatus`.
///
/// If `cal` is provided, raw probabilities are corrected via the inverse
/// confusion matrix built **only** from the qubits actually measured in this
/// circuit.  This prevents the O(4^n) memory explosion that occurs when the
/// full device calibration map (all device qubits) is used naively.
fn build_er_from_counts(
    task_id: &str,
    shots: usize,
    qubits: Vec<Qubit>,
    mut counts: HashMap<Outcome, usize>,
    cal: Option<&CalibrationMap>,
) -> Option<ExecutionResult> {
    if counts.is_empty() {
        return None;
    }
    let n = qubits.len();

    if let Some(cal_map) = cal {
        // Extract fidelities only for the qubits that were actually measured,
        // in the same order they appear in the measurement bitstring.
        // This keeps the confusion matrix at 2^n_measured × 2^n_measured instead
        // of 2^n_device × 2^n_device (which would be gigabytes for large chips).
        let local_f00: Vec<f64> = qubits
            .iter()
            .map(|q| {
                cal_map
                    .get(&(q.index() as u32))
                    .map(|&(f0, _)| f0)
                    .unwrap_or(1.0)
            })
            .collect();
        let local_f11: Vec<f64> = qubits
            .iter()
            .map(|q| {
                cal_map
                    .get(&(q.index() as u32))
                    .map(|&(_, f1)| f1)
                    .unwrap_or(1.0)
            })
            .collect();

        // Convert counts → bitstring-keyed probabilities → calibrate → back to counts.
        let total: usize = counts.values().sum();
        let inv = 1.0 / total as f64;
        let prob_str: HashMap<String, f64> = counts
            .iter()
            .map(|(o, &c)| (o.to_string(n), c as f64 * inv))
            .collect();
        let cal_probs = calibration::calibrate_probabilities(&prob_str, &local_f00, &local_f11);
        counts = cal_probs
            .into_iter()
            .filter_map(|(bitstr, p)| {
                let c = (p * shots as f64).round() as usize;
                if c == 0 {
                    return None;
                }
                Outcome::from_bitstring(&bitstr).ok().map(|o| (o, c))
            })
            .collect();
    }

    let mut er = ExecutionResult::new(
        task_id.to_string(),
        qubits,
        shots,
        n,
        None,
        Some(OffsetDateTime::now_utc()),
    );
    er.start(None);
    er.finish(counts, None);
    er.calc_probabilities();
    Some(er)
}

/// Fallback: build [`ExecutionResult`] from the `probability` field.
///
/// Counts are approximated by `round(prob × shots)`.  Use only when
/// `resultStatus` is unavailable.
fn build_er_from_prob_map(
    task_id: &str,
    shots: usize,
    prob_map: &HashMap<String, f64>,
) -> Option<ExecutionResult> {
    if prob_map.is_empty() {
        return None;
    }
    let num_qubits = prob_map.keys().next().map(|k| k.len()).unwrap_or(0);
    if num_qubits == 0 {
        return None;
    }
    let qubits: Vec<Qubit> = (0..num_qubits as u32).map(Qubit::new).collect();
    let mut counts: HashMap<Outcome, usize> = HashMap::new();
    for (bitstr, &prob) in prob_map {
        if let Ok(outcome) = Outcome::from_bitstring(bitstr) {
            let count = (prob * shots as f64).round() as usize;
            if count > 0 {
                counts.insert(outcome, count);
            }
        }
    }
    let mut er = ExecutionResult::new(
        task_id.to_string(),
        qubits,
        shots,
        num_qubits,
        None,
        Some(OffsetDateTime::now_utc()),
    );
    er.start(None);
    er.finish(counts, None);
    er.calc_probabilities();
    Some(er)
}

#[allow(dead_code)]
fn _unused_status_marker() {
    let _ = Status::Completed;
}

#[cfg(test)]
#[path = "task_test.rs"]
mod tests;
