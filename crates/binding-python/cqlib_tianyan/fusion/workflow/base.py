"""Fusion workflow base class."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Literal

from cqlib_tianyan.fusion.workflow.context import Context

StageKind = Literal["classic", "quantum"]
Decision = Literal[
    "next_classic",
    "next_quantum",
    "next_round",
    "finish",
    "fail",
]


@dataclass(frozen=True)
class StageSpec:
    kind: StageKind
    name: str
    method_name: str
    gate_set: tuple[str, ...] = ()
    logical_qubits: int | None = None
    estimated_physical_qubits: int | None = None


class FusionWorkflow:
    """Base class for user-defined hybrid workflows."""

    max_rounds: int = 100

    def start(self, ctx: Context) -> Context:
        """Optional hook before the first stage."""
        return ctx

    def decide(self, ctx: Context, last_kind: StageKind | None) -> Decision:
        """Return the next control action after a stage completes."""
        if ctx.converged:
            return "finish"
        if last_kind == "classic":
            return "next_quantum" if self.quantum_stages() else "finish"
        if last_kind == "quantum":
            if ctx.round + 1 >= self.max_rounds:
                return "finish"
            return "next_round"
        if self.classic_stages():
            return "next_classic"
        if self.quantum_stages():
            return "next_quantum"
        return "fail"

    def on_finish(self, ctx: Context) -> Context:
        """Optional hook when workflow completes."""
        return ctx

    def stages(self) -> list[StageSpec]:
        specs: list[StageSpec] = []
        methods: dict[str, Any] = {}
        # Class dictionaries preserve definition order. Walking the MRO from base
        # to subclass also makes overrides deterministic without relying on the
        # alphabetical ordering imposed by dir().
        for owner in reversed(type(self).__mro__):
            for name, raw_method in vars(owner).items():
                if name.startswith("_"):
                    continue
                methods[name] = raw_method
        for name in methods:
            method = getattr(self, name)
            kind = getattr(method, "_fusion_stage_kind", None)
            if kind not in ("classic", "quantum"):
                continue
            specs.append(
                StageSpec(
                    kind=kind,
                    name=getattr(method, "_fusion_stage_name", name),
                    method_name=name,
                    gate_set=tuple(getattr(method, "_fusion_gate_set", ())),
                    logical_qubits=getattr(method, "_fusion_logical_qubits", None),
                    estimated_physical_qubits=getattr(
                        method, "_fusion_estimated_physical_qubits", None
                    ),
                )
            )
        return specs

    def classic_stages(self) -> list[StageSpec]:
        return [s for s in self.stages() if s.kind == "classic"]

    def quantum_stages(self) -> list[StageSpec]:
        return [s for s in self.stages() if s.kind == "quantum"]

    def get_stage(self, kind: StageKind, index: int) -> StageSpec:
        items = self.classic_stages() if kind == "classic" else self.quantum_stages()
        if not items:
            raise ValueError(f"no {kind} stage registered")
        return items[index % len(items)]

    def run_stage(
        self, spec: StageSpec, ctx: Context
    ) -> tuple[Context, dict[str, Any]]:
        method = getattr(self, spec.method_name)
        result = method(ctx)
        payload: dict[str, Any]
        if isinstance(result, Context):
            ctx = result
            payload = dict(ctx.data)
        elif isinstance(result, dict):
            payload = result
            ctx = ctx.with_updates(**result)
        else:
            payload = {"value": result}
            ctx = ctx.with_updates(value=result)

        if spec.kind == "classic":
            ctx = Context(
                round=ctx.round,
                classic_iterations=ctx.classic_iterations + 1,
                quantum_iterations=ctx.quantum_iterations,
                data=ctx.data,
                last_classic_result=payload,
                last_quantum_result=ctx.last_quantum_result,
                converged=ctx.converged,
            )
        else:
            ctx = Context(
                round=ctx.round,
                classic_iterations=ctx.classic_iterations,
                quantum_iterations=ctx.quantum_iterations + 1,
                data=ctx.data,
                last_classic_result=ctx.last_classic_result,
                last_quantum_result=payload,
                converged=ctx.converged,
            )
        return ctx, payload
