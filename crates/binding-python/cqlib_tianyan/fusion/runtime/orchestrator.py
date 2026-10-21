"""Local fusion workflow orchestrator."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal

from cqlib_tianyan.fusion.checkpoint.store import CheckpointStore
from cqlib_tianyan.fusion.events.model import EventEmitter, FusionEvent
from cqlib_tianyan.fusion.exceptions import WorkflowError
from cqlib_tianyan.fusion.runtime.classic.local import LocalClassicRunner
from cqlib_tianyan.fusion.runtime.quantum.mock import MockQuantumRunner
from cqlib_tianyan.fusion.spec.manifest import FusionManifest
from cqlib_tianyan.fusion.workflow.base import Decision, FusionWorkflow, StageKind
from cqlib_tianyan.fusion.workflow.context import Context

StageIndex = dict[StageKind, int]


@dataclass
class RunResult:
    status: Literal["success", "failed"]
    context: Context
    events: list[FusionEvent] = field(default_factory=list)
    subtasks: list[dict[str, Any]] = field(default_factory=list)


class Orchestrator:
    """Drive a FusionWorkflow with classic/quantum runners."""

    def __init__(
        self,
        workflow: FusionWorkflow,
        manifest: FusionManifest,
        workdir: Path,
        *,
        classic_runner: LocalClassicRunner | None = None,
        quantum_runner: MockQuantumRunner | Any | None = None,
    ):
        self.workflow = workflow
        self.manifest = manifest
        self.workdir = workdir
        self.workdir.mkdir(parents=True, exist_ok=True)
        self.classic_runner = classic_runner or LocalClassicRunner()
        self.quantum_runner = quantum_runner or MockQuantumRunner()
        self.emitter = EventEmitter(workdir / "events.jsonl")
        self.checkpoint = CheckpointStore(workdir / "checkpoint")
        self.subtasks: list[dict[str, Any]] = []
        self._stage_index: StageIndex = {"classic": 0, "quantum": 0}

    def run(self) -> RunResult:
        saved = self.checkpoint.load_state() if self.manifest.runtime.checkpoint else None
        if saved and "context" in saved:
            ctx = Context.from_dict(saved["context"])
            saved_last_kind = saved.get("last_kind")
            last_kind: StageKind | None = (
                saved_last_kind if saved_last_kind in {"classic", "quantum"} else None
            )
            raw_index = saved.get("stage_index") or {}
            self._stage_index = {
                "classic": int(raw_index.get("classic", 0)),
                "quantum": int(raw_index.get("quantum", 0)),
            }
            self.subtasks = list(saved.get("subtasks") or [])
            self.emitter.emit(
                "orchestrator",
                "running",
                "workflow orchestrator resumed from checkpoint",
                round=ctx.round,
            )
        else:
            ctx = self.workflow.start(Context())
            last_kind = None
            self.emitter.emit("submit", "success", "fusion task accepted")
            self.emitter.emit("environment", "success", "local runtime ready")
            self.emitter.emit(
                "orchestrator",
                "running",
                "workflow orchestrator started",
                round=ctx.round,
            )

        status: Literal["success", "failed"] = "success"

        try:
            while True:
                if last_kind is None:
                    if self.workflow.classic_stages():
                        action: Decision = "next_classic"
                    elif self.workflow.quantum_stages():
                        action = "next_quantum"
                    else:
                        action = "fail"
                else:
                    self.emitter.emit(
                        "aggregate",
                        "success",
                        f"{last_kind} stage output aggregated",
                        round=ctx.round,
                        stage_kind=last_kind,
                    )
                    self.emitter.emit(
                        "state",
                        "success",
                        "workflow context updated",
                        round=ctx.round,
                    )
                    action = self.workflow.decide(ctx, last_kind)
                    self.emitter.emit(
                        "decision",
                        "success",
                        f"decision={action}",
                        round=ctx.round,
                    )

                    if action == "finish":
                        break
                    if action == "fail":
                        raise WorkflowError("workflow returned fail decision")
                    if action == "next_round":
                        ctx = Context(
                            round=ctx.round + 1,
                            classic_iterations=ctx.classic_iterations,
                            quantum_iterations=ctx.quantum_iterations,
                            data=ctx.data,
                            last_classic_result=ctx.last_classic_result,
                            last_quantum_result=ctx.last_quantum_result,
                            converged=ctx.converged,
                        )
                        last_kind = None
                        self._stage_index = {"classic": 0, "quantum": 0}
                        self.emitter.emit(
                            "orchestrator",
                            "running",
                            f"round {ctx.round} started",
                            round=ctx.round,
                        )
                        self._save_checkpoint(ctx, last_kind)
                        continue

                if action == "finish":
                    break
                if action == "fail":
                    raise WorkflowError("workflow returned fail decision")
                kind: StageKind = "quantum" if action == "next_quantum" else "classic"
                spec = self.workflow.get_stage(kind, self._stage_index[kind])
                self._stage_index[kind] += 1

                self.emitter.emit(
                    "orchestrator",
                    "running",
                    f"dispatch {kind} stage {spec.name}",
                    round=ctx.round,
                    stage_kind=kind,
                    stage_name=spec.name,
                )
                self.emitter.emit(
                    kind,
                    "running",
                    f"{kind} stage started",
                    round=ctx.round,
                    stage_kind=kind,
                    stage_name=spec.name,
                )

                if kind == "classic":
                    if hasattr(self.classic_runner, "run_stage"):
                        ctx, result = self.classic_runner.run_stage(
                            self.workflow, spec, ctx
                        )
                    else:  # Backward compatibility for third-party runners.
                        ctx, payload = self.workflow.run_stage(spec, ctx)
                        result = self.classic_runner.run(spec.name, payload)
                    self.subtasks.append(
                        {
                            "id": result.job_id,
                            "kind": "classic",
                            "stage": spec.name,
                            "status": "success",
                        }
                    )
                else:
                    ctx, payload = self.workflow.run_stage(spec, ctx)
                    result = self.quantum_runner.run(spec.name, payload)
                    ctx = ctx.with_updates(quantum_job=result.output)
                    ctx = Context(
                        round=ctx.round,
                        classic_iterations=ctx.classic_iterations,
                        quantum_iterations=ctx.quantum_iterations,
                        data=ctx.data,
                        last_classic_result=ctx.last_classic_result,
                        last_quantum_result=result.output,
                        converged=ctx.converged,
                    )
                    for query_id in result.output.get("task_ids") or [result.query_id]:
                        self.subtasks.append(
                            {
                                "id": str(query_id),
                                "kind": "quantum",
                                "stage": spec.name,
                                "status": "success",
                            }
                        )

                self.emitter.emit(
                    kind,
                    "success",
                    f"{kind} stage completed",
                    round=ctx.round,
                    stage_kind=kind,
                    stage_name=spec.name,
                )
                last_kind = kind
                self._save_checkpoint(ctx, last_kind)

            ctx = self.workflow.on_finish(ctx)
            self.emitter.emit("final", "success", "results materialized", round=ctx.round)
            self.emitter.emit("finish", "success", "workflow finished", round=ctx.round)
            if self.manifest.runtime.checkpoint:
                self.checkpoint.clear()

        except Exception as exc:  # noqa: BLE001
            status = "failed"
            self.emitter.emit("final", "failed", str(exc), round=ctx.round)
            self.emitter.emit("finish", "failed", "workflow terminated", round=ctx.round)
            raise WorkflowError(str(exc)) from exc

        return RunResult(
            status=status,
            context=ctx,
            events=list(self.emitter.events),
            subtasks=list(self.subtasks),
        )

    def _save_checkpoint(self, ctx: Context, last_kind: StageKind | None) -> None:
        if not self.manifest.runtime.checkpoint:
            return
        self.checkpoint.save_state(
            {
                "version": 1,
                "context": ctx.to_dict(),
                "last_kind": last_kind,
                "stage_index": dict(self._stage_index),
                "subtasks": list(self.subtasks),
            }
        )
