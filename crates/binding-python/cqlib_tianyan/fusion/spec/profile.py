"""Task profile for intelligent scheduling."""

from __future__ import annotations

from dataclasses import dataclass, field

from cqlib_tianyan.fusion.spec.manifest import FusionManifest
from cqlib_tianyan.fusion.workflow.base import FusionWorkflow


@dataclass
class TaskProfile:
    gate_set: list[str] = field(default_factory=list)
    logical_qubits: int | None = None
    estimated_physical_qubits: int | None = None
    mode: str = "generic"

    def to_dict(self) -> dict:
        return {
            "gate_set": self.gate_set,
            "logical_qubits": self.logical_qubits,
            "estimated_physical_qubits": self.estimated_physical_qubits,
            "mode": self.mode,
        }


def extract_profile(manifest: FusionManifest, workflow: FusionWorkflow | None = None) -> TaskProfile:
    """Merge manifest hints with stage decorator metadata."""
    gate_set = list(manifest.profile.gate_set)
    logical_qubits = manifest.profile.logical_qubits
    estimated_physical_qubits = manifest.profile.estimated_physical_qubits

    if workflow is not None:
        for stage in workflow.quantum_stages():
            gate_set.extend(stage.gate_set)
            if stage.logical_qubits is not None:
                logical_qubits = max(logical_qubits or 0, stage.logical_qubits)
            if stage.estimated_physical_qubits is not None:
                estimated_physical_qubits = max(
                    estimated_physical_qubits or 0,
                    stage.estimated_physical_qubits,
                )

    gate_set = sorted(set(g.upper() for g in gate_set if g))

    return TaskProfile(
        gate_set=gate_set,
        logical_qubits=logical_qubits,
        estimated_physical_qubits=estimated_physical_qubits,
        mode=manifest.mode,
    )
