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

//! Parse device calibration configuration from the Tianyan platform
//! into a [`cqlib_core::device::Device`] struct.
//!
//! The download-config endpoint returns a detailed JSON document containing
//! qubit frequencies, T1/T2 coherence times, gate errors, readout fidelities,
//! coupler topology, and disabled-qubit information.  This module deserialises
//! that JSON and builds a fully-populated `Device` that can be used for
//! transpilation, noise modelling, or display.

use crate::client::TianyanClient;
use crate::config::DOWNLOAD_CONFIG_PATH;
use crate::device::DeviceType;
use crate::error::TianyanError;
use cqlib_core::circuit::gate::instruction::Instruction;
use cqlib_core::circuit::gate::standard_gate::StandardGate;
use cqlib_core::device::{Device, EdgeProp, InstructionProp, PhysicalQubit, QubitProp, Topology};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

/// Top-level JSON returned by the download-config endpoint.
#[derive(Deserialize)]
struct RawConfig {
    #[serde(rename = "calibrationTime")]
    calibration_time: Option<String>,
    #[serde(rename = "computerId", default)]
    #[allow(dead_code)]
    computer_id: Option<String>,
    /// Comma-separated list of offline/faulty qubit names, e.g. `"Q2,Q10"`.
    #[serde(rename = "disabledQubits")]
    disabled_qubits: Option<String>,
    /// Comma-separated list of disabled coupler names, e.g. `"G0,G4,G7"`.
    #[serde(rename = "disabledCouplers")]
    disabled_couplers: Option<String>,
    overview: Option<Overview>,
    qubit: Option<QubitSection>,
    readout: Option<ReadoutSection>,
    #[serde(rename = "twoQubitGate")]
    two_qubit_gate: Option<TwoQubitGateSection>,
}

#[derive(Deserialize)]
struct Overview {
    qubits: Vec<String>,
    coupler_map: HashMap<String, Vec<String>>,
    #[serde(rename = "T1")]
    t1: Option<f64>,
    #[serde(rename = "T2")]
    t2: Option<f64>,
    cz_error: Option<f64>,
    #[serde(rename = "1q_gate_error")]
    single_qubit_error: Option<f64>,
    readout_error: Option<f64>,
}

/// Per-qubit calibration data block.
#[derive(Deserialize)]
struct QubitSection {
    frequency: Option<FrequencyBlock>,
    relatime: Option<RelatimeBlock>,
    #[serde(rename = "singleQubit")]
    single_qubit: Option<SingleQubitBlock>,
}

#[derive(Deserialize)]
struct FrequencyBlock {
    f01: Option<ParamArray>,
}

#[derive(Deserialize)]
struct RelatimeBlock {
    #[serde(rename = "T1")]
    t1: Option<ParamArray>,
    #[serde(rename = "T2")]
    t2: Option<ParamArray>,
}

#[derive(Deserialize)]
struct SingleQubitBlock {
    #[serde(rename = "gate error")]
    gate_error: Option<ParamArray>,
}

#[derive(Deserialize)]
struct ReadoutSection {
    #[serde(rename = "readoutArray")]
    readout_array: Option<ReadoutArray>,
}

#[derive(Deserialize)]
struct ReadoutArray {
    #[serde(rename = "Readout Error")]
    readout_error: Option<ParamArray>,
    #[serde(rename = "|0> readout fidelity")]
    fidelity_0: Option<ParamArray>,
    #[serde(rename = "|1> readout fidelity")]
    fidelity_1: Option<ParamArray>,
}

#[derive(Deserialize)]
struct TwoQubitGateSection {
    #[serde(rename = "czGate")]
    cz_gate: Option<CzGateBlock>,
}

#[derive(Deserialize)]
struct CzGateBlock {
    #[serde(rename = "gate error")]
    gate_error: Option<ParamArray>,
}

/// A common array structure: parallel arrays of values and qubit labels.
#[derive(Deserialize)]
struct ParamArray {
    param_list: Vec<f64>,
    qubit_used: Vec<String>,
}

/// Parse a qubit name like `"Q3"` into `PhysicalQubit::new(3)`.
fn parse_qubit(name: &str) -> Option<PhysicalQubit> {
    name.strip_prefix('Q')
        .and_then(|s| s.parse::<u32>().ok())
        .map(PhysicalQubit::new)
}

/// Build a lookup table from a [`ParamArray`]: qubit name → value.
fn param_map(arr: &ParamArray) -> HashMap<String, f64> {
    arr.qubit_used
        .iter()
        .zip(arr.param_list.iter())
        .map(|(q, &v)| (q.clone(), v))
        .collect()
}

/// Parse a calibration time string like `"2025-08-07 15:16:22"` into an
/// [`OffsetDateTime`](time::OffsetDateTime).
fn parse_calibration_time(s: &str) -> Option<time::OffsetDateTime> {
    let format = time::format_description::parse_borrowed::<3>(
        "[year]-[month]-[day] [hour]:[minute]:[second]",
    )
    .ok()?;
    time::PrimitiveDateTime::parse(s, &format)
        .ok()
        .map(|dt| dt.assume_utc())
}

/// Download and parse the calibration configuration for a device, returning
/// a fully-populated [`Device`].
///
/// This calls the `/qccp-quantum/sdk/experiment/download/config/{machine}` endpoint.
/// Only superconducting devices support configuration downloads.
pub fn download_device_config(
    client: &TianyanClient,
    machine: &str,
) -> Result<Device, TianyanError> {
    let (device, _calibration) = download_device_config_full(client, machine)?;
    Ok(device)
}

/// Download and parse the calibration configuration, returning both the
/// [`Device`] and optional [`ReadoutCalibrationData`] in a single network call.
/// Rejects non-superconducting devices before sending a request.
///
/// The platform sometimes returns the calibration JSON as a **string** inside
/// the `data` envelope (double-encoded JSON). This function handles both cases,
/// mirroring the Python reference implementation:
/// ```python
/// cfg = result.get('data')
/// if isinstance(cfg, str):
///     cfg = json.loads(cfg)
/// ```
pub fn download_device_config_full(
    client: &TianyanClient,
    machine: &str,
) -> Result<(Device, Option<ReadoutCalibrationData>), TianyanError> {
    if DeviceType::from_code(machine) != Some(DeviceType::Superconducting) {
        return Err(TianyanError::InvalidInput(format!(
            "device configuration download is only supported for superconducting devices: {machine}"
        )));
    }
    let path = format!("{}/{}", DOWNLOAD_CONFIG_PATH, machine);
    let resp: crate::client::ApiResponse<serde_json::Value> = client.get(&path)?;
    let raw_value = resp.into_data()?;

    // The API sometimes returns the calibration JSON as a string (double-encoded).
    // Decode it if needed, just like the Python reference code does.
    let config_value: serde_json::Value = if let Some(s) = raw_value.as_str() {
        serde_json::from_str(s).map_err(TianyanError::Json)?
    } else {
        raw_value
    };

    let device = parse_device_config(machine, &config_value)?;
    let calibration = extract_readout_calibration(&config_value);
    Ok((device, calibration))
}

/// Parse a raw JSON value into a [`Device`].
///
/// Exposed for testing and for callers who already have the JSON.
///
/// # Qubit and coupler filtering
///
/// The platform JSON has two categories of disabled hardware:
/// - `disabledQubits`  — qubits that are offline or faulty; excluded from the
///   topology and recorded in [`Device::invalid_qubits`].
/// - `disabledCouplers` — couplers (edges) that are currently non-functional;
///   excluded from the topology edges even if both endpoint qubits are available.
///
/// `Device::new` enforces that every qubit appearing in the topology must also
/// be in the "available qubits" set, so disabled qubits must be removed from
/// topology nodes before construction.
pub fn parse_device_config(
    machine: &str,
    json: &serde_json::Value,
) -> Result<Device, TianyanError> {
    let raw: RawConfig = serde_json::from_value(json.clone()).map_err(TianyanError::Json)?;

    let overview = raw.overview.ok_or_else(|| {
        TianyanError::InvalidInput("config JSON missing 'overview' section".into())
    })?;

    // These must be resolved before constructing the topology so that
    // Device::new's invariant (topology qubits ⊆ available qubits) is satisfied.

    let disabled_qubit_set: HashSet<PhysicalQubit> = raw
        .disabled_qubits
        .as_deref()
        .map(|s| {
            s.split(',')
                .filter_map(|name| parse_qubit(name.trim()))
                .collect()
        })
        .unwrap_or_default();

    let disabled_coupler_set: HashSet<&str> = raw
        .disabled_couplers
        .as_deref()
        .map(|s| s.split(',').map(str::trim).collect())
        .unwrap_or_default();

    // Disabled qubits are excluded from the topology entirely; they are recorded
    // separately in Device::invalid_qubits for informational purposes.

    let all_qubit_set: HashSet<PhysicalQubit> = overview
        .qubits
        .iter()
        .filter_map(|name| parse_qubit(name))
        .collect();
    let invalid_qubit_set: HashSet<PhysicalQubit> = disabled_qubit_set
        .iter()
        .filter(|qubit| all_qubit_set.contains(qubit))
        .copied()
        .collect();

    let available_qubits: Vec<PhysicalQubit> = overview
        .qubits
        .iter()
        .filter_map(|name| parse_qubit(name))
        .filter(|q| !disabled_qubit_set.contains(q))
        .collect();

    // A coupler is excluded if:
    //   (a) its name appears in `disabledCouplers`, or
    //   (b) either of its endpoint qubits is disabled (the disabled-coupler list
    //       from the API should already cover these, but we guard defensively).

    let mut coupling_entries: Vec<(PhysicalQubit, PhysicalQubit, String)> = Vec::new();
    for (coupler_name, qubit_pair) in &overview.coupler_map {
        if disabled_coupler_set.contains(coupler_name.as_str()) {
            continue;
        }
        if qubit_pair.len() == 2 {
            if let (Some(q0), Some(q1)) = (parse_qubit(&qubit_pair[0]), parse_qubit(&qubit_pair[1]))
            {
                // Defensively skip edges whose endpoints are disabled.
                if disabled_qubit_set.contains(&q0) || disabled_qubit_set.contains(&q1) {
                    continue;
                }
                coupling_entries.push((q0, q1, coupler_name.clone()));
            }
        }
    }

    // Topology is built from available qubits only; Device::new validates that
    // every topology node is in the provided qubit set.

    let topology = Topology::new(available_qubits, coupling_entries)
        .map_err(|e| TianyanError::InvalidInput(format!("failed to build topology: {}", e)))?;

    let mut device = Device::new(machine, all_qubit_set, topology)
        .map_err(|e| TianyanError::InvalidInput(format!("failed to build device: {}", e)))?
        .with_native_gates(native_gates_for_machine(machine));

    // Record disabled qubits separately so callers can inspect them.
    if !invalid_qubit_set.is_empty() {
        device = device.with_invalid_qubits(invalid_qubit_set).map_err(|e| {
            TianyanError::InvalidInput(format!("failed to set invalid qubits: {}", e))
        })?;
    }

    if let Some(ref ct) = raw.calibration_time {
        if let Some(dt) = parse_calibration_time(ct) {
            device = device.with_calibration_time(dt);
        }
    }

    if let Some(t1) = overview.t1 {
        device = device.with_default_t1(t1);
    }
    if let Some(t2) = overview.t2 {
        device = device.with_default_t2(t2);
    }
    if let Some(re) = overview.readout_error {
        device = device.with_default_readout_error(re / 100.0);
    }
    if let Some(sqe) = overview.single_qubit_error {
        device = device.with_default_single_qubit_error(sqe / 100.0);
    }
    if let Some(cze) = overview.cz_error {
        device = device.with_default_two_qubit_error(cze / 100.0);
    }

    // Collect per-qubit data from various sub-sections into lookup maps.
    let freq_map = raw
        .qubit
        .as_ref()
        .and_then(|q| q.frequency.as_ref())
        .and_then(|f| f.f01.as_ref())
        .map(param_map)
        .unwrap_or_default();

    let t1_map = raw
        .qubit
        .as_ref()
        .and_then(|q| q.relatime.as_ref())
        .and_then(|r| r.t1.as_ref())
        .map(param_map)
        .unwrap_or_default();

    let t2_map = raw
        .qubit
        .as_ref()
        .and_then(|q| q.relatime.as_ref())
        .and_then(|r| r.t2.as_ref())
        .map(param_map)
        .unwrap_or_default();

    let sq_error_map = raw
        .qubit
        .as_ref()
        .and_then(|q| q.single_qubit.as_ref())
        .and_then(|sq| sq.gate_error.as_ref())
        .map(param_map)
        .unwrap_or_default();

    let readout_err_map = raw
        .readout
        .as_ref()
        .and_then(|r| r.readout_array.as_ref())
        .and_then(|a| a.readout_error.as_ref())
        .map(param_map)
        .unwrap_or_default();

    let fidelity_0_map = raw
        .readout
        .as_ref()
        .and_then(|r| r.readout_array.as_ref())
        .and_then(|a| a.fidelity_0.as_ref())
        .map(param_map)
        .unwrap_or_default();

    let fidelity_1_map = raw
        .readout
        .as_ref()
        .and_then(|r| r.readout_array.as_ref())
        .and_then(|a| a.fidelity_1.as_ref())
        .map(param_map)
        .unwrap_or_default();

    // Build QubitProp for each qubit that has calibration data.
    for qubit_name in &overview.qubits {
        let qubit = match parse_qubit(qubit_name) {
            Some(q) => q,
            None => continue,
        };

        // Readout error (% → fraction). Default to 0.0 if missing.
        let readout_err = readout_err_map
            .get(qubit_name)
            .map(|v| v / 100.0)
            .unwrap_or(0.0);

        let mut prop = QubitProp::new(readout_err);

        // Readout fidelities → confusion matrix entries
        // |0⟩ readout fidelity = P(measure 0 | prepared 0) = F00
        // |1⟩ readout fidelity = P(measure 1 | prepared 1) = F11
        // prob_meas0_prep1 = P(measure 0 | prepared 1) = 1 - F11
        // prob_meas1_prep0 = P(measure 1 | prepared 0) = 1 - F00
        if let Some(&f00) = fidelity_0_map.get(qubit_name) {
            prop = prop.with_prob_meas1_prep0(1.0 - f00);
        }
        if let Some(&f11) = fidelity_1_map.get(qubit_name) {
            prop = prop.with_prob_meas0_prep1(1.0 - f11);
        }

        if let Some(&t1) = t1_map.get(qubit_name) {
            prop = prop.with_t1(t1);
        }
        if let Some(&t2) = t2_map.get(qubit_name) {
            prop = prop.with_t2(t2);
        }
        if let Some(&freq) = freq_map.get(qubit_name) {
            prop = prop.with_frequency(freq);
        }

        // Single-qubit gate error → InstructionProp for X2P (√X gate)
        if let Some(&err_pct) = sq_error_map.get(qubit_name) {
            let gate_prop =
                InstructionProp::new(Instruction::Standard(StandardGate::X2P), err_pct / 100.0);
            prop = prop.with_native_instruction(gate_prop);
        }

        // Ignore errors for qubits that don't exist in topology
        let _ = device.add_qubit_properties(qubit, prop);
    }

    if let Some(ref tqg) = raw.two_qubit_gate {
        if let Some(ref cz) = tqg.cz_gate {
            if let Some(ref cz_err) = cz.gate_error {
                let cz_map = param_map(cz_err);
                for (coupler_name, &err_pct) in &cz_map {
                    // Look up this coupler in the overview's coupler_map to find qubit pair.
                    if let Some(pair) = overview.coupler_map.get(coupler_name) {
                        if pair.len() == 2 {
                            if let (Some(control), Some(target)) =
                                (parse_qubit(&pair[0]), parse_qubit(&pair[1]))
                            {
                                let edge_prop =
                                    EdgeProp::new().with_native_instruction(InstructionProp::new(
                                        Instruction::Standard(StandardGate::CZ),
                                        err_pct / 100.0,
                                    ));
                                let _ = device.add_edge_properties(control, target, edge_prop);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(device)
}

fn native_gates_for_machine(machine: &str) -> Vec<Instruction> {
    let mut gates = vec![
        Instruction::Standard(StandardGate::RZ),
        Instruction::Standard(StandardGate::X2P),
        Instruction::Standard(StandardGate::X2M),
        Instruction::Standard(StandardGate::Y2P),
        Instruction::Standard(StandardGate::Y2M),
    ];

    match machine {
        "tianyan-287" | "tianyan176" | "tianyan176-2" | "tianyan504" => {
            gates.push(Instruction::Standard(StandardGate::XY2P));
            gates.push(Instruction::Standard(StandardGate::XY2M));
        }
        _ => {}
    }

    gates.push(Instruction::Standard(StandardGate::CZ));

    if machine == "tianyan504" {
        gates.push(Instruction::Standard(StandardGate::FSIM));
    }

    gates
}

/// Readout calibration data extracted from the device config for use in
/// measurement error mitigation.
///
/// Each qubit has `f00` (P(0|0)) and `f11` (P(1|1)) fidelities.
#[derive(Debug, Clone)]
pub struct ReadoutCalibrationData {
    /// Qubit names in order, e.g. `["Q0", "Q1", ...]`.
    pub qubit_names: Vec<String>,
    /// P(measure 0 | prepared 0) per qubit.
    pub f00: Vec<f64>,
    /// P(measure 1 | prepared 1) per qubit.
    pub f11: Vec<f64>,
}

impl ReadoutCalibrationData {
    /// Build a lookup map from qubit hardware index to `(f00, f11)` fidelity pair.
    ///
    /// Qubit names like `"Q8"` are parsed to hardware index `8`.  Entries whose
    /// name cannot be parsed are silently skipped.
    ///
    /// This map is used internally to look up fidelities only for the qubits that
    /// were actually measured in a circuit, avoiding the need to build a full
    /// 2^n × 2^n confusion matrix over all device qubits.
    pub fn to_cal_map(&self) -> HashMap<u32, (f64, f64)> {
        self.qubit_names
            .iter()
            .zip(self.f00.iter().zip(self.f11.iter()))
            .filter_map(|(name, (&f0, &f1))| {
                name.strip_prefix('Q')
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(|idx| (idx, (f0, f1)))
            })
            .collect()
    }
}

/// Extract readout calibration data from the raw config JSON.
///
/// Returns `None` if the required fidelity arrays are not present.
pub fn extract_readout_calibration(json: &serde_json::Value) -> Option<ReadoutCalibrationData> {
    let raw: RawConfig = serde_json::from_value(json.clone()).ok()?;
    let readout = raw.readout?.readout_array?;
    let fid_0 = readout.fidelity_0?;
    let fid_1 = readout.fidelity_1?;

    if fid_0.qubit_used.len() != fid_0.param_list.len()
        || fid_1.qubit_used.len() != fid_1.param_list.len()
    {
        return None;
    }

    // Both arrays should reference the same qubits. Use fid_0's order.
    let fid_1_map: HashMap<String, f64> =
        fid_1.qubit_used.into_iter().zip(fid_1.param_list).collect();

    let mut qubit_names = Vec::new();
    let mut f00 = Vec::new();
    let mut f11 = Vec::new();

    for (name, &val_0) in fid_0.qubit_used.iter().zip(fid_0.param_list.iter()) {
        if let Some(&val_1) = fid_1_map.get(name) {
            qubit_names.push(name.clone());
            f00.push(val_0);
            f11.push(val_1);
        }
    }

    Some(ReadoutCalibrationData {
        qubit_names,
        f00,
        f11,
    })
}

#[cfg(test)]
#[path = "device_config_test.rs"]
mod tests;
