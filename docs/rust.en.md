# cqlib-tianyan — Rust Tutorial

A complete guide to using the **cqlib-tianyan** Rust library to authenticate with, submit quantum circuits to, and retrieve calibrated results from the **Tianyan Quantum Cloud Platform** (`qc.zdxlz.com`).

---

## Table of Contents

1. [Installation](#1-installation)
2. [Authentication](#2-authentication)
3. [Listing & Inspecting Backends](#3-listing--inspecting-backends)
4. [Submitting Circuits](#4-submitting-circuits)
5. [Retrieving Results](#5-retrieving-results)
6. [Readout Error Calibration](#6-readout-error-calibration)
7. [Batch Submission](#7-batch-submission)
8. [Advanced: CalibrationMode](#8-advanced-calibrationmode)
9. [Error Handling](#9-error-handling)

---

## 1. Installation

```sh
cargo add cqlib-tianyan
```

The library uses **synchronous (blocking) I/O** via `reqwest::blocking` — no Tokio runtime is needed.

---

## 2. Authentication

### 2.1 First-time login

Call `TianyanPlatform::login` with your API key (OpenID). By default, the resulting credentials are stored in `~/.cqlib/tianyan/credentials.json` (macOS/Linux) or `%APPDATA%\cqlib\tianyan\credentials.json` (Windows) so they can be reused later.

```rust
use cqlib_tianyan::TianyanPlatform;

let api_key = std::env::var("TIANYAN_API_KEY").expect("set TIANYAN_API_KEY");
let platform = TianyanPlatform::login(&api_key)?;
```

### 2.2 Reuse saved credentials

On subsequent runs you can skip re-logging in. If the token is expired, the library automatically refreshes it:

```rust
let platform = TianyanPlatform::from_credentials()?;
```

### 2.3 Custom authentication options

Use `TianyanConfig` to customise saving, refresh, and path behaviour:

```rust
use cqlib_tianyan::{TianyanPlatform, config::TianyanConfig};

// In-memory only — never write credentials to disk
let cfg = TianyanConfig::default()
    .with_save_credentials(false)
    .with_auto_refresh(false);

let platform = TianyanPlatform::login_with_config(&api_key, cfg)?;
```

| Option | Default | Description |
|--------|---------|-------------|
| `with_save_credentials(bool)` | `true` | Persist token to disk after login/refresh |
| `with_auto_refresh(bool)` | `true` | Re-login automatically when token expires |
| `with_credentials_path(path)` | `~/.cqlib/tianyan/credentials.json` | Override the file path |

### 2.4 Load from a custom path

```rust
use std::path::PathBuf;
use cqlib_tianyan::{TianyanPlatform, config::TianyanConfig};

let cfg = TianyanConfig::default()
    .with_credentials_path(PathBuf::from("/secure/vault/creds.json"));

let platform = TianyanPlatform::from_credentials_with_config(cfg)?;
```

---

## 3. Listing & Inspecting Backends

### 3.1 List all backends

```rust
let backends = platform.list_backends()?;

for b in &backends {
    println!("{:30} type={:?} status={:?}", b.name, b.device_type, b.status);
}
```

`backend.device_type` returns the backend's `DeviceType`.

| `DeviceType` | Devices |
|---|---|
| `Simulator` | `tianyan_sw`, `tianyan_s`, `tianyan_tn`, `tianyan_tnn`, `tianyan_sa`, `tianyan_swn` |
| `Superconducting` | `tianyan176`, `tianyan176-2`, `tianyan24`, `tianyan504`, `tianyan-287`, `tianyan-294` |
| `Photonic` | `tianyan-p2000` |

Only `Superconducting` devices support configuration downloads and readout calibration; qubit-count queries that depend on configuration have the same restriction.

Only `Superconducting` and `Simulator` devices can submit tasks; `Photonic` and `IonTrap` submissions fail locally.

`Auto` skips configuration download and calibration for non-superconducting devices and returns raw counts. Explicitly selecting `Enabled` on a non-superconducting device fails before submission. `Disabled` always returns raw counts. `is_available` reports running status only; it does not guarantee submission support.

### 3.2 Select a specific backend

```rust
let backend = platform.get_backend("tianyan-287")?;
println!("Selected: {} ({:?})", backend.name, backend.status);
```

### 3.3 Inspect the device topology

Configuration access is available only for superconducting devices. The device calibration configuration includes the qubit/coupler topology and measured hardware properties:

```rust
println!("Physical qubits: {}", backend.num_qubits()?);

backend.with_device(|device| {
    let topo = device.topology();
    println!("Available topology qubits: {}", topo.num_qubits());
    println!("Couplings: {}", topo.num_couplings());

    if let Some(t) = device.calibration_time() {
        println!("Calibrated at: {}", t);
    }
})?;
```

---

## 4. Submitting Circuits

Circuits are expressed as **QCIS strings**. Separate gates and measurements with `\n`.

### 4.1 Bell state example

```rust
use std::time::Duration;

// Bell state |Φ+⟩ = (|00⟩ + |11⟩) / √2
// CZ is the native gate; CNOT(ctrl=Q1, tgt=Q8) = H(Q8) · CZ(Q1,Q8) · H(Q8)
// H Q1    — superpose control qubit
// H Q8    — basis change (start CNOT decomposition)
// CZ Q1 Q8 — native two-qubit gate
// H Q8    — basis change back (complete CNOT)
// M Q1 / M Q8 — measure both qubits
let circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8";
let shots = 1000;

let task = backend.run(vec![circuit.into()], shots)?;
println!("Task IDs: {:?}", task.task_ids());
```

`backend.run()` returns a [`TaskHandle`] immediately. The circuit is queued on the cloud; results are fetched via polling.

### 4.2 Using `Circuit` objects from cqlib-core

`CircuitInput` also accepts `cqlib_core::circuit::Circuit` objects directly:

```rust
use cqlib_tianyan::device::CircuitInput;
use cqlib_core::circuit::Circuit;

let circuit: Circuit = /* build programmatically */;
let task = backend.run(vec![CircuitInput::from(circuit)], 1000)?;
```

---

## 5. Retrieving Results

### 5.1 Blocking wait

Poll until all circuits complete (or timeout):

```rust
use std::time::Duration;

let results = task.wait(
    Duration::from_secs(120), // maximum wait
    Duration::from_secs(5),   // poll interval
)?;

for r in &results {
    println!("Task : {}", r.task_id());
    println!("Counts: {:?}", r.counts());
    if let Some(probs) = r.probabilities() {
        println!("Probs : {:?}", probs);
    }
}
```

**Default behaviour**: `wait()` applies readout error calibration automatically on superconducting devices when calibration data is available and at most 14 qubits are measured (see §6). To always get raw counts:

```rust
let raw_results = task.wait_raw(Duration::from_secs(120), Duration::from_secs(5))?;
```

### 5.2 Non-blocking status check

```rust
let partial = task.status()?; // may be fewer than submitted circuits
println!("{}/{} complete", partial.len(), task.task_ids().len());
```

---

## 6. Readout Error Calibration

Quantum measurements are imperfect. For each qubit `k` in state `|0⟩` (or `|1⟩`), the hardware may misread with probability `ε₀ₖ` (or `ε₁ₖ`). The calibration procedure measures:

| Fidelity | Symbol | Meaning |
|----------|--------|---------|
| `f00[k]` | F(0\|0) | P(measure 0 \| prepared 0) |
| `f11[k]` | F(1\|1) | P(measure 1 \| prepared 1) |

### 6.1 The confusion matrix

For a single qubit, the **confusion matrix** `A_k` is:

```
         Measured 0      Measured 1
Prep 0 [  f00[k]         1 - f00[k]  ]
Prep 1 [  1 - f11[k]     f11[k]      ]
```

`A_k` maps true state probabilities to observed probabilities:

```
p_observed = A_k · p_true
```

### 6.2 Multi-qubit calibration (Kronecker product)

For an N-qubit system, the full confusion matrix is the **tensor (Kronecker) product** of per-qubit matrices:

```
A = A_{N-1} ⊗ A_{N-2} ⊗ … ⊗ A_0
```

This produces a 2ᴺ × 2ᴺ matrix where rows are "prepared" bitstrings and columns are "measured" bitstrings.

### 6.3 Mitigation via matrix inversion

The **inverse confusion matrix** `A⁻¹` maps observed probabilities back to estimated true probabilities:

```
p_true ≈ A⁻¹ · p_observed
```

The implementation:
1. Builds the 2×2 inverse for each qubit: `A_k⁻¹ = 1/(f00+f11-1) · [[f11, f11-1], [f00-1, f00]]`
2. Takes the Kronecker product over all measured qubits.
3. Applies the full inverse to the measured probability vector.
4. Clamps negative values to 0 and renormalises so probabilities sum to 1.

### 6.4 Reading calibration data

```rust
if let Some(cal) = backend.readout_calibration_data()? {
    for (i, name) in cal.qubit_names.iter().enumerate() {
        let f00 = cal.f00[i];
        let f11 = cal.f11[i];
        println!("{}: f00={:.4}  f11={:.4}  err={:.2}%",
            name, f00, f11, (1.0 - (f00 + f11) / 2.0) * 100.0);
    }
}
```

### 6.5 Auto-calibration (recommended)

`wait()` uses the `CalibrationMode::Auto` strategy (default). It downloads configuration only for superconducting devices and applies calibration when **both** conditions are met:

1. Calibration data is available on the backend.
2. The circuit measures **≤ `AUTO_CALIBRATION_MAX_QUBITS` (14) qubits**.

> **Why the 14-qubit limit?**
> The inverse confusion matrix requires O(4ⁿ) memory where n is the number of measured qubits. At n = 14 that is ~2 GiB; at n = 15 it is ~8 GiB. Above this threshold `Auto` silently falls back to raw counts to prevent out-of-memory crashes. Use `CalibrationMode::Enabled` if you need to force calibration on larger circuits and have sufficient RAM.

```rust
// CalibrationMode::Auto is the default — auto-calibrates for ≤ 14 measured qubits:
let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
```

### 6.6 Manual calibration with explicit fidelities

Supply a `ReadoutCalibrationData` value obtained from `backend.readout_calibration_data()`.
Only the fidelities for the qubits actually measured in each circuit are used; the library
filters by hardware qubit index automatically.

```rust
use cqlib_tianyan::device_config::ReadoutCalibrationData;

if let Some(cal) = backend.readout_calibration_data()? {
    let results = task.wait_with_calibration(
        Duration::from_secs(120),
        Duration::from_secs(5),
        &cal,
    )?;
}
```

### 6.7 Comparing calibrated vs. raw

```rust
let cal_r = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
let raw_r = task.wait_raw(Duration::from_secs(120), Duration::from_secs(5))?;

// Compare outcome probabilities
for (cr, rr) in cal_r.iter().zip(raw_r.iter()) {
    println!("Calibrated: {:?}", cr.probabilities());
    println!("Raw       : {:?}", rr.probabilities());
}
```

---

## 7. Batch Submission

The Tianyan platform allows up to **50 circuits per submission request**. `cqlib-tianyan` handles splitting automatically:

```rust
// 120 circuits — automatically split into 3 batches of 50, 50, 20
let circuits: Vec<_> = (0..120)
    .map(|_| "H Q0\nM Q0".into())
    .collect();

let task = backend.run(circuits, 1000)?;
// task.task_ids().len() == 120
```

Results are returned in the same order as the submitted circuits.

---

## 8. Advanced: CalibrationMode

`CalibrationMode` controls how `wait()` handles readout mitigation:

| Variant | Behaviour |
|---------|-----------|
| `Auto` *(default)* | On superconducting devices, apply calibration when data is available **and** the circuit measures ≤ 14 qubits; silently fall back to raw counts otherwise |
| `Enabled` | Require a superconducting device and calibration data, regardless of qubit count; reject other types before submission (caller is responsible for sufficient RAM) |
| `Disabled` | Always return raw counts; no mitigation applied |

> **The 14-qubit threshold**: the inverse confusion matrix needs O(4ⁿ) memory; for n > 14 this exceeds 2 GiB, so `Auto` falls back automatically. See `AUTO_CALIBRATION_MAX_QUBITS`.

Set at submission time:

```rust
use cqlib_tianyan::task::CalibrationMode;

// Require calibration — fail if data is missing
let task = backend.run_with_mode(circuits, 1000, CalibrationMode::Enabled)?;

// Skip calibration entirely
let task = backend.run_raw(circuits, 1000)?;

// Or change after construction:
let mut task = backend.run(circuits, 1000)?;
task.calibration_mode = CalibrationMode::Disabled;
let results = task.wait(timeout, interval)?; // raw
```

---

## 9. Error Handling

All fallible operations return `Result<T, TianyanError>`. The error enum variants:

| Variant | Cause |
|---------|-------|
| `Auth(msg)` | Login failed or token invalid |
| `Http(err)` | Network or HTTP-level failure |
| `Json(err)` | Unexpected response format |
| `DeviceNotFound(name)` | No backend with the given name |
| `InvalidInput(msg)` | Bad arguments (empty circuit list, shots=0, etc.) |
| `Timeout(duration)` | `wait()` exceeded `timeout` before results arrived |
| `Io(err)` | Credential file read/write failure |

```rust
use cqlib_tianyan::TianyanError;

match platform.get_backend("nonexistent") {
    Err(TianyanError::DeviceNotFound(name)) => {
        eprintln!("Backend '{}' not found — check the device list.", name);
    }
    Err(e) => return Err(e),
    Ok(backend) => { /* ... */ }
}
```

---

## Complete Example

```rust
use cqlib_tianyan::{TianyanPlatform, TianyanError};
use std::time::Duration;

fn main() -> Result<(), TianyanError> {
    // 1. Authenticate (saves credentials to ~/.cqlib/tianyan/)
    let api_key = std::env::var("TIANYAN_API_KEY").expect("set TIANYAN_API_KEY");
let platform = TianyanPlatform::login(&api_key)?;

    // 2. Select backend
    let backend = platform.get_backend("tianyan-287")?;

    // 3. Inspect calibration fidelities
    if let Some(cal) = backend.readout_calibration_data()? {
        println!("Readout fidelities:");
        for (i, q) in cal.qubit_names.iter().enumerate() {
            println!("  {} f00={:.4} f11={:.4}", q, cal.f00[i], cal.f11[i]);
        }
    }

    // 4. Submit Bell-state circuit (Q1 ↔ Q8 are coupled on tianyan-287)
    let task = backend.run(
        vec!["H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8".into()],
        1000,
    )?;

    // 5. Wait and print calibrated results (default CalibrationMode::Auto)
    let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
    for r in &results {
        println!("Counts: {:?}", r.counts());
    }

    Ok(())
}
```
