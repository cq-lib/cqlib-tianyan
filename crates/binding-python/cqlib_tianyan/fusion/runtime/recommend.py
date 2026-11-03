"""Local TOP-N resource recommendations for interactive confirmation."""

from __future__ import annotations

from dataclasses import dataclass

from cqlib_tianyan.fusion.client.models import ResourcePlan
from cqlib_tianyan.fusion.spec.profile import TaskProfile

Preference = str

_WEIGHTS = {
    "balanced": (0.45, 0.30, 0.25),
    "quality": (0.70, 0.15, 0.15),
    "speed": (0.20, 0.65, 0.15),
    "cost": (0.20, 0.15, 0.65),
}

_GATES = frozenset({"CZ", "RZ", "SX", "X", "H"})


@dataclass(frozen=True)
class _Candidate:
    machine: str
    quantum: str
    hpc_code: str
    hpc: str
    physical_qubits: int
    quality: float
    wait_min: float
    cost: float
    reliability: str


_CATALOG = (
    _Candidate(
        "tianyan176",
        "天衍-176",
        "cloud-hpc-batch",
        "Cloud HPC / batch",
        176,
        84,
        3,
        57,
        "中高",
    ),
    _Candidate(
        "tianyan287",
        "天衍-287",
        "cloud-hpc-batch",
        "Cloud HPC / batch",
        287,
        88,
        7,
        72,
        "高",
    ),
    _Candidate(
        "tianyan504",
        "天衍-504",
        "local-hpc",
        "Local HPC / local",
        504,
        81,
        1,
        50,
        "中高",
    ),
    _Candidate(
        "tianyan176",
        "天衍-176",
        "local-hpc",
        "Local HPC / local",
        176,
        82,
        5,
        37,
        "中高",
    ),
    _Candidate(
        "tianyan287",
        "天衍-287",
        "cloud-hpc-gpu",
        "Cloud HPC / GPU",
        287,
        91,
        16,
        94,
        "高",
    ),
    _Candidate(
        "tianyan504",
        "天衍-504",
        "cloud-hpc-batch",
        "Cloud HPC / batch",
        504,
        81,
        1,
        60,
        "中高",
    ),
)


def recommend_local(
    profile: TaskProfile,
    *,
    preference: Preference = "balanced",
    quantum_hint: str | None = None,
    hpc_hint: str | None = None,
    limit: int = 3,
) -> list[ResourcePlan]:
    """Rank resource pairs that can run this task profile."""
    needed = profile.estimated_physical_qubits or profile.logical_qubits or 0
    required = {gate.upper() for gate in profile.gate_set}
    eligible = [
        item
        for item in _CATALOG
        if item.physical_qubits >= needed and required <= _GATES
    ]
    if not eligible:
        return []

    waits = [item.wait_min for item in eligible]
    costs = [item.cost for item in eligible]
    quality_weight, speed_weight, cost_weight = _WEIGHTS.get(
        preference, _WEIGHTS["balanced"]
    )
    ranked: list[tuple[float, _Candidate]] = []
    for item in eligible:
        speed = _inverse_score(item.wait_min, min(waits), max(waits))
        cost = _inverse_score(item.cost, min(costs), max(costs))
        score = (
            quality_weight * item.quality
            + speed_weight * speed
            + cost_weight * cost
            + _hint_bonus(item, quantum_hint, hpc_hint)
        )
        ranked.append((score, item))
    ranked.sort(key=lambda pair: (-pair[0], pair[1].machine, pair[1].hpc_code))

    plans: list[ResourcePlan] = []
    for rank, (score, item) in enumerate(ranked[:limit], start=1):
        plans.append(
            ResourcePlan(
                id=f"{item.machine}-{item.hpc_code}",
                quantum_resource=item.quantum,
                hpc_resource=item.hpc,
                rank=rank,
                score=f"{score:.1f}",
                quality=f"{item.quality:.0f}%",
                wait=f"{item.wait_min:.0f} 分钟",
                cost=f"约 {item.cost:.0f} 元",
                reliability=item.reliability,
                raw={
                    "machine": item.machine,
                    "hpc_code": item.hpc_code,
                    "preference": preference,
                },
            )
        )
    return plans


def _inverse_score(value: float, low: float, high: float) -> float:
    if high <= low:
        return 100.0
    return 100.0 * (high - value) / (high - low)


def _hint_bonus(
    item: _Candidate, quantum_hint: str | None, hpc_hint: str | None
) -> float:
    bonus = 0.0
    if quantum_hint and quantum_hint.strip().lower() in {
        item.machine,
        item.quantum.lower(),
    }:
        bonus += 6.0
    if hpc_hint and hpc_hint.strip().lower() in {item.hpc_code, item.hpc.lower()}:
        bonus += 4.0
    return bonus
