"""Local mock quantum runner."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass
class QuantumJobResult:
    query_id: str
    output: dict[str, Any]


class MockQuantumRunner:
    """Simulates QPU submission without contacting the cloud."""

    def __init__(self, prefix: str = "Q"):
        self._counter = 0
        self.prefix = prefix

    def run(self, stage_name: str, payload: dict[str, Any]) -> QuantumJobResult:
        self._counter += 1
        shots = payload.get("shots", 1000)
        return QuantumJobResult(
            query_id=f"{self.prefix}-{self._counter:04d}",
            output={
                "stage": stage_name,
                "engine": "mock-quantum",
                "shots": shots,
                "counts": {"0": shots // 2, "1": shots - shots // 2},
            },
        )
