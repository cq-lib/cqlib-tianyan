# cqlib-tianyan

Python bindings for the Tianyan quantum cloud platform client.

## Overview

This package provides synchronous Python access to the Tianyan quantum computing cloud platform, allowing users to:

- Authenticate with API keys
- Discover available quantum backends
- Submit quantum circuits (QCIS format)
- Retrieve execution results with readout error mitigation

All blocking operations release the GIL, making the library safe for use in multi-threaded Python applications.

## Installation

### From Source (Development)

```bash
# Install maturin if not already installed
pip install maturin

# Build and install the package
maturin develop
```

### Production Build

```bash
maturin build --release
pip install target/wheels/*.whl
```

## Quick Start

```python
from cqlib_tianyan import TianyanPlatform

# Authenticate with your API key
platform = TianyanPlatform.login("your_api_key")

# List available backends
backends = platform.list_backends()
for backend in backends:
    print(f"{backend.name}: {backend.status}")

# Get a specific backend
backend = platform.get_backend("tianyan-287")
print(f"Physical qubits: {backend.num_qubits()}")

# Submit a circuit
task = backend.run(["H Q1\nM Q1"], shots=1000)

# Wait for results
results = task.wait(timeout=120.0)
for result in results:
    print(f"Task ID: {result.task_id}")
    print(f"Counts: {result.counts}")
    print(f"Probabilities: {result.probabilities}")
```

## API Reference

### Core Classes

#### `TianyanPlatform`

The main entry point for interacting with the Tianyan platform.

```python
# Login with API key
platform = TianyanPlatform.login(
    api_key="your_key",
    domain="qc.zdxlz.com",  # Optional
    save_credentials=True,   # Optional, default: True
    auto_refresh=True,       # Optional, default: True
)

# Load from saved credentials
platform = TianyanPlatform.from_credentials()

# List all backends
backends = platform.list_backends()

# Get specific backend
backend = platform.get_backend("device_name")

# Submit circuits directly
 task = platform.submit(
    circuits=["H Q1\nM Q1"],
    shots=1000,
    device_name="tianyan-287"
)
```

#### `TianyanBackend`

Represents a quantum computing backend.

Only superconducting devices and simulators can submit tasks. Photonic and ion-trap
devices fail before submission. Configuration access (`device_config()` and
`num_qubits()`) and readout calibration require a superconducting device.
In `"auto"` mode, simulators return raw counts without downloading configuration;
superconducting devices apply calibration when data is available and at most 14
qubits are measured. Explicit `"enabled"` mode fails before submission on any
non-superconducting device. `"disabled"` always returns raw counts.

```python
# Backend properties
backend.name           # Device identifier
backend.display_name   # Human-readable name
backend.device_type    # DeviceType enum
backend.status         # DeviceStatus enum
backend.toll           # DeviceToll enum (free/paid)
if backend.device_type == "superconducting":
    backend.num_qubits()  # Physical qubit count (loads config on first use)

# Check availability
if backend.is_available() and backend.device_type.value in ("superconducting", "simulator"):
    # Submit circuits
    task = backend.run(circuits=["H Q1\nM Q1"], shots=1000)

    # Raw execution (no calibration)
    task = backend.run(circuits=["H Q1\nM Q1"], shots=1000, calibration_mode="disabled")

    # With explicit calibration mode
    task = backend.run(
        circuits=["H Q1\nM Q1"],
        shots=1000,
        calibration_mode="disabled"  # "auto", "enabled", "disabled"
    )

    # Get device configuration
    if backend.device_type == "superconducting":
        device = backend.device_config()  # Returns cqlib.device.Device
```

`run(circuits, shots, *, calibration_mode="auto")` accepts a string or a
`CalibrationMode` object. `platform.submit()` accepts the same keyword-only
option. Each call submits a new task. `run_raw()` and `run_with_mode()` remain
deprecated aliases and emit `DeprecationWarning`.

#### `TaskHandle`

Represents a batch of submitted circuits.

`wait()` and `wait_raw()` take `timeout` and `poll_interval` in seconds, with
defaults of `120.0` and `5.0`. Both methods can be called without arguments.

```python
# Task properties
task.task_ids       # List of platform-assigned task IDs
task.device_name    # Backend name
task.shots          # Number of shots
task.submitted_at   # ISO 8601 timestamp

# Non-blocking status check
results = task.status()  # Returns completed results only

# Blocking wait for all results
results = task.wait(
    timeout=120.0,
    poll_interval=5.0
)

# Wait for raw results
results = task.wait_raw()  # timeout=120.0, poll_interval=5.0 (seconds)
```

#### `TianyanConfig`

Configuration for the platform client.

```python
config = TianyanConfig(
    domain="qc.zdxlz.com",
    save_credentials=True,
    auto_refresh=True,
    credentials_path="~/.cqlib/tianyan/credentials.json"
)
```

#### Enums

```python
from cqlib_tianyan import DeviceStatus, DeviceToll, DeviceType, CalibrationMode

# DeviceStatus values: "running", "calibration", "under_maintenance", "offline", "upgrading", "unknown"
# DeviceToll values: "free", "paid", "unknown"
# DeviceType values: "superconducting", "photonic", "ion_trap", "simulator"
# CalibrationMode values: "auto", "enabled", "disabled"
```

Read `backend.device_type.value`, or compare directly with a string, such as
`backend.device_type == "simulator"`.

| Device type | `.value` | Devices |
|---|---|---|
| Simulator | `simulator` | `tianyan_sw`, `tianyan_s`, `tianyan_tn`, `tianyan_tnn`, `tianyan_sa`, `tianyan_swn` |
| Photonic | `photonic` | `tianyan-p2000` |
| Superconducting | `superconducting` | `tianyan176`, `tianyan176-2`, `tianyan24`, `tianyan504`, `tianyan-287`, `tianyan-294` |

## Exception Handling

```python
from cqlib_tianyan import TianyanError

try:
    platform = TianyanPlatform.login("invalid_key")
except Exception as e:
    # Handle authentication failure
    print(f"Login failed: {e}")
```

**Note:** In abi3 mode, `TianyanError` cannot be caught by type. Catch `Exception` instead.

## Testing

### Test Structure

```
tests/
├── conftest.py              # Pytest configuration and fixtures
└── test_cqlib_tianyan.py    # Main test file
```

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

# Run integration tests only
TIANYAN_API_KEY="your_key" pytest tests/ -v -m integration

# Include slow tests (long timeouts)
TIANYAN_API_KEY="your_key" pytest tests/ -v --run-slow

# Run specific test class
TIANYAN_API_KEY="your_key" pytest tests/test_cqlib_tianyan.py::TestCircuitSubmission -v
```

### Test Markers

| Marker | Description |
|--------|-------------|
| `integration` | Requires API access and network connectivity |
| `slow` | Long-running tests (waits for circuit execution) |

## Technical Details

### Architecture

This package is built with:

- **PyO3**: Rust-Python bindings
- **Maturin**: Build tool for Python-Rust hybrid projects
- **abi3**: Stable Python ABI (supports Python 3.10+)

### Module Structure

```
crates/binding-python/
├── Cargo.toml           # Rust package manifest
├── pyproject.toml       # Python package configuration
├── src/
│   ├── lib.rs          # Module initialization
│   ├── error.rs        # Exception types
│   ├── config.rs       # TianyanConfig bindings
│   ├── platform.rs     # TianyanPlatform bindings
│   ├── backend.rs      # TianyanBackend bindings
│   └── task.rs         # TaskHandle bindings
├── cqlib_tianyan/
│   ├── __init__.py     # Python package entry
│   ├── __init__.pyi    # Type stubs
│   └── py.typed        # PEP 561 marker
└── tests/
    ├── conftest.py     # Pytest configuration
    └── test_cqlib_tianyan.py  # Test suite
```

### Known Limitations

1. **ABI3 Mode**: Due to `abi3-py310` feature:
   - `TianyanError` cannot inherit from `RuntimeError`
   - `TianyanError` cannot be instantiated directly in Python
   - Exception catching must use `except Exception:` instead of `except TianyanError:`

2. **GIL Release**: Long-running operations (`wait()`, `wait_raw()`) release the GIL to allow other Python threads to run.

## Dependencies

- Python >= 3.10
- Optional extra `cqlib>=1.4.0b1` (`pip install cqlib-tianyan[cqlib]`) for
  `device_config()` and `ExecutionResult` conversion. Do not install classic
  PyPI `cqlib 1.3.x`; it does not provide `cqlib.device` or `cqlib.circuit`.
- `cqlib-core` (Rust workspace / git dependency)
- `cqlib-tianyan` (Rust workspace dependency)

## License

Licensed under the Apache License, Version 2.0.

Copyright (c) 2026 China Telecom Quantum Group
