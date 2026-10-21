"""Local mock classic runner."""

from __future__ import annotations

from dataclasses import dataclass
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from cqlib_tianyan.fusion.workflow.base import FusionWorkflow, StageSpec
    from cqlib_tianyan.fusion.workflow.context import Context


@dataclass
class ClassicJobResult:
    job_id: str
    output: dict[str, Any]


class LocalClassicRunner:
    """Simulates HPC/Slurm execution in-process."""

    def __init__(self, prefix: str = "C"):
        self._counter = 0
        self.prefix = prefix

    def run(self, stage_name: str, payload: dict[str, Any]) -> ClassicJobResult:
        self._counter += 1
        return ClassicJobResult(
            job_id=f"{self.prefix}-{self._counter:04d}",
            output={
                "stage": stage_name,
                "engine": "local-classic",
                "input_keys": sorted(payload.keys()),
            },
        )

    def run_stage(
        self,
        workflow: FusionWorkflow,
        spec: StageSpec,
        ctx: Context,
    ) -> tuple[Context, ClassicJobResult]:
        """Execute a classic stage locally while preserving its real output."""
        self._counter += 1
        next_ctx, payload = workflow.run_stage(spec, ctx)
        return next_ctx, ClassicJobResult(
            job_id=f"{self.prefix}-{self._counter:04d}",
            output=payload,
        )
