"""Minimal hybrid iterative fusion sample."""

from __future__ import annotations

from cqlib_tianyan.fusion import Context, FusionWorkflow, classic_stage, quantum_stage


class IterativeDemo(FusionWorkflow):
    """Classic optimization followed by quantum measurement, repeated."""

    @classic_stage(name="optimize_parameters")
    def optimize(self, ctx: Context) -> dict:
        energy = float(ctx.data.get("energy", 1.0))
        energy *= 0.55
        return {"energy": energy, "theta": 0.1 * (ctx.round + 1)}

    @quantum_stage(
        name="execute_circuit",
        gate_set=["CZ", "RZ", "SX"],
        logical_qubits=4,
        estimated_physical_qubits=6,
    )
    def execute(self, ctx: Context) -> dict:
        theta = ctx.data.get("theta", 0.0)
        qcis = f"RX Q0 {theta:.4f}\nM Q0"
        return {"qcis": qcis, "shots": 1024}

    def decide(self, ctx: Context, last_kind: str | None) -> str:
        if last_kind == "classic":
            return "next_quantum"
        energy = float(ctx.data.get("energy", 1.0))
        if energy < 0.25:
            ctx.converged = True
            return "finish"
        if ctx.round + 1 >= self.max_rounds:
            return "finish"
        return "next_round"


app = IterativeDemo()
