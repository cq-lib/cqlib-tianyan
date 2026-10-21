"""Long-session workflow with checkpoint-friendly iterations."""

from __future__ import annotations

from cqlib_tianyan.fusion import Context, FusionWorkflow, classic_stage, quantum_stage


class RuntimeDemo(FusionWorkflow):
    @classic_stage(name="session_update")
    def update_session(self, ctx: Context) -> dict:
        session_energy = float(ctx.data.get("session_energy", 1.0)) * 0.9
        return {
            "session_energy": session_energy,
            "session_id": ctx.data.get("session_id", "sess-001"),
            "step": ctx.round,
        }

    @quantum_stage(
        name="session_measure",
        gate_set=["RZ", "SX"],
        logical_qubits=6,
    )
    def measure(self, ctx: Context) -> dict:
        step = ctx.data.get("step", 0)
        qcis = f"RX Q0 {0.05 * (step + 1):.4f}\nM Q0"
        return {"qcis": qcis, "shots": 4096, "runtime": True}

    def decide(self, ctx: Context, last_kind: str | None) -> str:
        if last_kind == "classic":
            return "next_quantum"
        if float(ctx.data.get("session_energy", 1.0)) < 0.35:
            ctx.converged = True
            return "finish"
        if ctx.round + 1 >= self.max_rounds:
            return "finish"
        return "next_round"


app = RuntimeDemo()
