"""Generic single-pass classic then quantum workflow."""

from __future__ import annotations

from cqlib_tianyan.fusion import Context, FusionWorkflow, classic_stage, quantum_stage


class LegacyDemo(FusionWorkflow):
    @classic_stage(name="legacy_preprocess")
    def preprocess(self, ctx: Context) -> dict:
        return {"theta": 0.42, "legacy": True}

    @quantum_stage(name="legacy_execute", gate_set=["RZ", "X"], logical_qubits=2)
    def execute(self, ctx: Context) -> dict:
        theta = ctx.data.get("theta", 0.0)
        qcis = f"RX Q0 {theta:.4f}\nM Q0"
        return {"qcis": qcis, "shots": 512}

    def decide(self, ctx: Context, last_kind: str | None) -> str:
        if last_kind == "classic":
            return "next_quantum"
        ctx.converged = True
        return "finish"


app = LegacyDemo()
