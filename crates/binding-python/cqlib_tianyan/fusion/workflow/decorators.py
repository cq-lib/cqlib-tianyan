"""Stage decorators for fusion workflows."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any


def classic_stage(
    *,
    name: str | None = None,
) -> Callable[[Callable[..., Any]], Callable[..., Any]]:
    """Mark a method as a classic compute stage."""

    def decorator(func: Callable[..., Any]) -> Callable[..., Any]:
        func._fusion_stage_kind = "classic"  # type: ignore[attr-defined]
        func._fusion_stage_name = name or func.__name__  # type: ignore[attr-defined]
        return func

    return decorator


def quantum_stage(
    *,
    name: str | None = None,
    gate_set: list[str] | None = None,
    logical_qubits: int | None = None,
    estimated_physical_qubits: int | None = None,
) -> Callable[[Callable[..., Any]], Callable[..., Any]]:
    """Mark a method as a quantum compute stage."""

    def decorator(func: Callable[..., Any]) -> Callable[..., Any]:
        func._fusion_stage_kind = "quantum"  # type: ignore[attr-defined]
        func._fusion_stage_name = name or func.__name__  # type: ignore[attr-defined]
        func._fusion_gate_set = [g.upper() for g in (gate_set or [])]  # type: ignore[attr-defined]
        func._fusion_logical_qubits = logical_qubits  # type: ignore[attr-defined]
        func._fusion_estimated_physical_qubits = estimated_physical_qubits  # type: ignore[attr-defined]
        return func

    return decorator
