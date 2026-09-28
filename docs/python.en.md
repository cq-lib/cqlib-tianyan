# cqlib-tianyan — Python Tutorial

A complete guide to using the **cqlib-tianyan** Python library to authenticate with, submit quantum circuits to, and retrieve calibrated results from the **Tianyan Quantum Cloud Platform** (`qc.zdxlz.com`).

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
10. [Running Tests](#10-running-tests)

---

## 1. Installation

### Install via pip (Recommended)

```bash
pip install cqlib-tianyan
```

**Requirements**: Python >= 3.10

### From Source (Development)

```bash
# Install maturin
pip install maturin

# Build and install
cd crates/binding-python
maturin develop
```

---

## 2. Authentication

### 2.1 First-time login

Call `TianyanPlatform.login` with your API key (OpenID). By default, the resulting credentials are stored in `~/.cqlib/tianyan/credentials.json` (macOS/Linux) or `%APPDATA%\cqlib\tianyan\credentials.json` (Windows) so they can be reused later.

```python
import os
from cqlib_tianyan import TianyanPlatform

api_key = os.environ["TIANYAN_API_KEY"]
platform = TianyanPlatform.login(api_key)
```

### 2.2 Reuse saved credentials

On subsequent runs you can skip re-logging in. If the token is expired, the library automatically refreshes it:

```python
platform = TianyanPlatform.from_credentials()
```

### 2.3 Custom authentication options

Use keyword arguments to customise saving, refresh, and path behaviour:

```python
# In-memory only — never write credentials to disk
platform = TianyanPlatform.login(
    api_key,
    save_credentials=False,
    auto_refresh=False,
)
```

| Option | Default | Description |
|--------|---------|-------------|
| `domain` | `"qc.zdxlz.com"` | Platform hostname |
| `save_credentials` | `True` | Persist token to disk after login/refresh |
| `auto_refresh` | `True` | Re-login automatically when token expires |
| `credentials_path` | `~/.cqlib/tianyan/credentials.json` | Override the file path |

### 2.4 Load from a custom path

```python
platform = TianyanPlatform.from_credentials(
    credentials_path="/secure/vault/creds.json"
)
```

---

## 3. Listing & Inspecting Backends

### 3.1 List all backends

```python
backends = platform.list_backends()

for b in backends:
    print(f"{b.name:30} type={b.device_type} status={b.status}")
```

`backend.device_type` returns the backend's `DeviceType`.
Read its `.value` string or compare it directly, for example `backend.device_type == "simulator"`.

| Device type | `.value` | Devices |
|---|---|---|
| Simulator | `simulator` | `tianyan_sw`, `tianyan_s`, `tianyan_tn`, `tianyan_tnn`, `tianyan_sa`, `tianyan_swn` |
| Photonic | `photonic` | `tianyan-p2000` |
| Superconducting | `superconducting` | `tianyan176`, `tianyan176-2`, `tianyan24`, `tianyan504`, `tianyan-287`, `tianyan-294` |

Only `Superconducting` devices support configuration downloads and readout calibration; qubit-count queries that depend on configuration have the same restriction.

Only `Superconducting` and `Simulator` devices can submit tasks; `Photonic` and `IonTrap` submissions fail locally.

`Auto` skips configuration download and calibration for non-superconducting devices and returns raw counts. Explicitly selecting `Enabled` on a non-superconducting device fails before submission. `Disabled` always returns raw counts. `is_available` reports running status only; it does not guarantee submission support.

### 3.2 Select a specific backend

```python
backend = platform.get_backend("tianyan-287")
print(f"Selected: {backend.name} ({backend.status})")
```

### 3.3 Inspect the device topology

Configuration access is available only for superconducting devices. The device calibration configuration includes the qubit/coupler topology and measured hardware properties:

```python
device = backend.device_config()
topo = device.topology

print(f"Physical qubits: {backend.num_qubits()}")
print(f"Available topology qubits: {topo.num_qubits}")
print(f"Couplings: {topo.num_couplings}")

if device.calibration_time:
    print(f"Calibrated at: {device.calibration_time}")
```

---

## 4. Submitting Circuits

Circuits are expressed as **QCIS strings**. Separate gates and measurements with `\n`.

### 4.1 Bell state example

```python
# Bell state |Φ+⟩ = (|00⟩ + |11⟩) / √2
circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"
shots = 1000

task = backend.run([circuit], shots)
print(f"Task IDs: {task.task_ids}")
```

`backend.run()` returns a `TaskHandle` immediately. The circuit is queued on the cloud; results are fetched via polling.

### 4.2 Submit via platform shortcut

You can also submit directly via the platform object without fetching a backend first:

```python
task = platform.submit(
    circuits=[circuit],
    shots=1000,
    device_name="tianyan-287"
)
```

---

## 5. Retrieving Results

### 5.1 Blocking wait

Poll until all circuits complete (or timeout):

Both `wait()` and `wait_raw()` accept `timeout` and `poll_interval` in seconds,
defaulting to `120.0` and `5.0`. Call `task.wait()` or `task.wait_raw()` to use these defaults.

```python
results = task.wait(
    timeout=120,  # maximum wait (seconds)
    poll_interval=5  # poll interval (seconds)
)

for r in results:
    print(f"Task : {r.task_id}")
    print(f"Counts: {r.counts}")
    if r.probabilities:
        print(f"Probs : {r.probabilities}")
```

**Default behaviour**: `wait()` applies readout error calibration automatically on superconducting devices when calibration data is available and at most 14 qubits are measured (see §6). To always get raw counts:

```python
raw_results = task.wait_raw(timeout=120, poll_interval=5)
```

### 5.2 Non-blocking status check

```python
partial = task.status()  # may be fewer than submitted circuits
print(f"{len(partial)}/{len(task.task_ids)} complete")
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

### 6.4 Auto-calibration (recommended)

`wait()` uses the `CalibrationMode.Auto` strategy (default). It downloads configuration only for superconducting devices and applies calibration when **both** conditions are met:

1. Calibration data is available on the backend.
2. The circuit measures **≤ 14 qubits**.

> **Why the 14-qubit limit?**
> The inverse confusion matrix requires O(4ⁿ) memory, where n is the number of measured qubits. At n = 14 that is ~2 GiB; at n = 15 it is ~8 GiB. Above this threshold `Auto` silently falls back to raw counts to prevent out-of-memory crashes. Use `CalibrationMode.Enabled` if you need to force calibration on larger circuits and have sufficient RAM.

```python
# Default CalibrationMode.Auto — auto-calibrates for ≤ 14 measured qubits
results = task.wait(timeout=120, poll_interval=5)
```

### 6.5 Comparing calibrated vs. raw

```python
cal_results = task.wait(timeout=120, poll_interval=5)
raw_results = task.wait_raw(timeout=120, poll_interval=5)

for cr, rr in zip(cal_results, raw_results):
    print(f"Calibrated: {cr.probabilities}")
    print(f"Raw       : {rr.probabilities}")
```

---

## 7. Batch Submission

The Tianyan platform allows up to **50 circuits per submission request**. `cqlib-tianyan` handles splitting automatically:

```python
# 120 circuits — automatically split into 3 batches of 50, 50, 20
circuits = ["H Q0\nM Q0"] * 120

task = backend.run(circuits, 1000)
# len(task.task_ids) == 120
```

Results are returned in the same order as the submitted circuits.

---

## 8. Advanced: CalibrationMode

`CalibrationMode` controls how `wait()` handles readout mitigation:

| Variant | Behaviour |
|---------|-----------|
| `"auto"` *(default)* | On superconducting devices, apply calibration when data is available **and** the circuit measures ≤ 14 qubits; silently fall back to raw counts otherwise |
| `"enabled"` | Require a superconducting device and calibration data, regardless of qubit count; reject other types before submission (caller is responsible for sufficient RAM) |
| `"disabled"` | Always return raw counts; no mitigation applied |

> **The 14-qubit threshold for `auto`**: confusion matrix memory is O(4ⁿ); for n > 14 this exceeds 2 GiB, so `auto` falls back automatically to prevent OOM.

Set at submission time — accepts either a plain string or a `CalibrationMode` object:

`run(circuits, shots, *, calibration_mode="auto")` takes the calibration policy
as a keyword-only argument. `platform.submit()` accepts the same option. Each
call below submits a new task. The legacy `run_raw()` and `run_with_mode()`
methods remain deprecated aliases and emit `DeprecationWarning`.

```python
from cqlib_tianyan import CalibrationMode

circuits = ["H Q1\nM Q1"]

# Require calibration — fail if data is missing
task = backend.run(circuits, 1000, calibration_mode="enabled")

# Skip calibration
task = backend.run(circuits, 1000, calibration_mode="disabled")

# Pass a CalibrationMode object (equivalent to the string)
mode = CalibrationMode("disabled")
task = backend.run(circuits, 1000, calibration_mode=mode)  # object accepted directly

# CalibrationMode supports equality comparison with strings
assert mode == "disabled"                    # True
assert mode == CalibrationMode("disabled")   # True
```

---

## 9. Error Handling

All fallible operations raise exceptions. **Note**: In abi3 mode, `TianyanError` cannot be caught by type. Catch `Exception` instead:

```python
try:
    platform = TianyanPlatform.login("invalid_key")
except Exception as e:
    print(f"Login failed: {e}")
```

Common error scenarios:

| Scenario | Error Message |
|----------|---------------|
| Login failed or token invalid | Authentication error |
| Network or HTTP error | Connection timeout, etc. |
| Backend not found | Device does not exist |
| Invalid arguments | Empty circuit list, shots=0, etc. |
| Timeout | `wait()` exceeded `timeout` before results arrived |

---

## 10. Running Tests

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `TIANYAN_API_KEY` | For integration tests | Your Tianyan platform API key |
| `TIANYAN_DOMAIN` | Optional | Custom domain (defaults to qc.zdxlz.com) |
| `TIANYAN_DEVICE` | Optional | Target device name (e.g., "tianyan-287") |

### Running Tests

```bash
# Run all tests (requires API key)
TIANYAN_API_KEY="your_key" pytest tests/ -v

# Run with specific device
TIANYAN_API_KEY="your_key" TIANYAN_DEVICE="tianyan-287" pytest tests/ -v

# Run only unit tests (no API key required)
pytest tests/ -v -m "not integration"

# Include slow tests (long timeouts)
TIANYAN_API_KEY="your_key" PYTEST_RUN_SLOW=1 pytest tests/ -v
```

---

## Complete Example

```python
import os
from cqlib_tianyan import TianyanPlatform, CalibrationMode

# 1. Authenticate (saves credentials to ~/.cqlib/tianyan/)
api_key = os.environ["TIANYAN_API_KEY"]
platform = TianyanPlatform.login(api_key)

# 2. Select backend
backend = platform.get_backend("tianyan-287")

# 3. Inspect device info
print(f"Device: {backend.name}")
print(f"Status: {backend.status}")
print(f"Physical qubits: {backend.num_qubits()}")

# 4. Submit Bell-state circuit (Q1 ↔ Q8 are coupled on tianyan-287)
circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8"
task = backend.run([circuit], 1000)

# 5. Wait and print calibrated results (default CalibrationMode.Auto)
results = task.wait(timeout=120, poll_interval=5)
for r in results:
    print(f"Task ID: {r.task_id}")
    print(f"Counts: {r.counts}")
    print(f"Probabilities: {r.probabilities}")
```
