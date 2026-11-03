"""Fusion cloud API models."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Literal

FusionStatus = Literal[
    "wait",
    "awaiting_confirm",
    "queued",
    "running",
    "success",
    "fail",
    "failed",
    "paused",
    "resumed",
    "cancelled",
    "unknown",
]


_V2_STATUSES = {
    1: "wait",
    2: "queued",
    3: "paused",
    4: "resumed",
    5: "cancelled",
    6: "success",
    7: "failed",
    8: "running",
    9: "unknown",
}


def _optional_int(value: Any) -> int | None:
    return None if value is None else int(value)


@dataclass
class FusionJob:
    id: str
    name: str
    status: FusionStatus = "wait"
    dispatch_mode: str | None = None
    dispatch_preference: str | None = None
    quantum_resource: str | None = None
    hpc_resource: str | None = None
    classic_iterations: int | None = None
    quantum_iterations: int | None = None
    package_name: str | None = None
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_api(cls, payload: dict[str, Any]) -> FusionJob:
        return cls(
            id=str(payload.get("task_id") or payload.get("id") or ""),
            name=str(
                payload.get("name")
                or payload.get("task_name")
                or payload.get("taskName")
                or ""
            ),
            status=_V2_STATUSES.get(
                payload.get("status"), str(payload.get("status") or "unknown")
            ),  # type: ignore[arg-type]
            dispatch_mode=payload.get("dispatch_mode") or payload.get("schedulingMode"),
            dispatch_preference=payload.get("dispatch_preference")
            or payload.get("schedulingPreference"),
            quantum_resource=payload.get("quantum_resource")
            or payload.get("qpuResourceCode")
            or payload.get("qpu"),
            hpc_resource=payload.get("hpc_resource")
            or payload.get("cpuResourceCode")
            or payload.get("hpc"),
            classic_iterations=_optional_int(
                payload.get("classicIterationCount", payload.get("classic_iterations"))
            ),
            quantum_iterations=_optional_int(
                payload.get("quantumIterationCount", payload.get("quantum_iterations"))
            ),
            package_name=payload.get("package_name") or payload.get("taskFileName"),
            raw=dict(payload),
        )


@dataclass
class ResourcePlan:
    """One recommended quantum + classic resource pair."""

    id: str
    quantum_resource: str
    hpc_resource: str
    rank: int = 1
    score: str | None = None
    quality: str | None = None
    wait: str | None = None
    execute: str | None = None
    cost: str | None = None
    reliability: str | None = None
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_api(cls, payload: dict[str, Any], *, rank: int = 1) -> ResourcePlan:
        return cls(
            id=str(payload.get("plan_id") or payload.get("id") or ""),
            quantum_resource=str(
                payload.get("quantum_resource") or payload.get("qpu") or ""
            ),
            hpc_resource=str(payload.get("hpc_resource") or payload.get("hpc") or ""),
            rank=int(payload.get("rank") or rank),
            score=_optional_str(payload.get("score")),
            quality=_optional_str(payload.get("quality")),
            wait=_optional_str(payload.get("wait")),
            execute=_optional_str(payload.get("execute")),
            cost=_optional_str(payload.get("cost")),
            reliability=_optional_str(payload.get("reliability")),
            raw=dict(payload),
        )


def _optional_str(value: Any) -> str | None:
    if value is None:
        return None
    return str(value)


@dataclass
class FusionSubTask:
    id: str
    kind: str
    stage: str
    status: str
    resource: str | None = None
    settled: bool | None = None
    raw: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_api(cls, payload: dict[str, Any]) -> FusionSubTask:
        return cls(
            id=str(payload.get("id") or payload.get("task_id") or ""),
            kind=str(payload.get("kind") or payload.get("type") or ""),
            stage=str(payload.get("stage") or payload.get("stage_name") or ""),
            status=str(payload.get("status") or ""),
            resource=payload.get("resource"),
            settled=payload.get("settled"),
            raw=dict(payload),
        )
