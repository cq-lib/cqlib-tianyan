"""Workflow execution context passed between stages."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class Context:
    round: int = 0
    classic_iterations: int = 0
    quantum_iterations: int = 0
    data: dict[str, Any] = field(default_factory=dict)
    last_classic_result: dict[str, Any] | None = None
    last_quantum_result: dict[str, Any] | None = None
    converged: bool = False

    def with_updates(self, **kwargs: Any) -> Context:
        new_data = dict(self.data)
        new_data.update(kwargs)
        return Context(
            round=self.round,
            classic_iterations=self.classic_iterations,
            quantum_iterations=self.quantum_iterations,
            data=new_data,
            last_classic_result=self.last_classic_result,
            last_quantum_result=self.last_quantum_result,
            converged=self.converged,
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "round": self.round,
            "classic_iterations": self.classic_iterations,
            "quantum_iterations": self.quantum_iterations,
            "data": self.data,
            "last_classic_result": self.last_classic_result,
            "last_quantum_result": self.last_quantum_result,
            "converged": self.converged,
        }

    @classmethod
    def from_dict(cls, payload: dict[str, Any]) -> Context:
        return cls(
            round=int(payload.get("round", 0)),
            classic_iterations=int(payload.get("classic_iterations", 0)),
            quantum_iterations=int(payload.get("quantum_iterations", 0)),
            data=dict(payload.get("data") or {}),
            last_classic_result=payload.get("last_classic_result"),
            last_quantum_result=payload.get("last_quantum_result"),
            converged=bool(payload.get("converged", False)),
        )
