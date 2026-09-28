"""Interface check for the Rust-backed cqlib companion package."""

from __future__ import annotations

import importlib.util

import pytest


def test_installed_cqlib_exposes_device_and_circuit() -> None:
    """Fail if an installed cqlib lacks the modules this binding calls."""
    if importlib.util.find_spec("cqlib") is None:
        pytest.skip("companion cqlib is not installed")
    try:
        import cqlib.circuit
        import cqlib.device
    except ModuleNotFoundError as exc:
        pytest.fail(
            "installed cqlib does not provide cqlib.device / cqlib.circuit "
            f"({exc}). Need the Rust-backed package (>=2.0.0b1), not 1.3.x."
        )
    assert hasattr(cqlib.device, "Device")
    assert hasattr(cqlib.device, "ExecutionResult")
    assert hasattr(cqlib.circuit, "Instruction")
    assert hasattr(cqlib.circuit, "StandardGate")
