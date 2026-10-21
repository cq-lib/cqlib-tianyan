"""Batch quantum subtasks within each fusion round."""

from __future__ import annotations

from cqlib_tianyan.fusion import Context, FusionWorkflow, classic_stage, quantum_stage


class BatchDemo(FusionWorkflow):
    batch_size = 3

    @classic_stage(name="prepare_batches")
    def prepare(self, ctx: Context) -> dict:
        return {"batch_count": self.batch_size, "batch_idx": 0, "values": list(range(self.batch_size))}

    @quantum_stage(
        name="batch_execute",
        gate_set=["CZ", "RZ", "SX"],
        logical_qubits=8,
        estimated_physical_qubits=12,
    )
    def execute(self, ctx: Context) -> dict:
        idx = int(ctx.data.get("batch_idx", 0))
        values = ctx.data.get("values", [])
        angle = values[idx] if idx < len(values) else idx
        qcis = f"RX Q0 {angle * 0.1:.4f}\nM Q0"
        return {"qcis": qcis, "shots": 2048, "batch_idx": idx}

    def decide(self, ctx: Context, last_kind: str | None) -> str:
        if last_kind == "classic":
            ctx.data["batch_idx"] = 0
            return "next_quantum"
        if last_kind == "quantum":
            idx = int(ctx.data.get("batch_idx", 0)) + 1
            if idx < self.batch_size:
                ctx.data["batch_idx"] = idx
                return "next_quantum"
            if ctx.round + 1 >= self.max_rounds:
                return "finish"
            return "next_round"
        return "next_classic"


app = BatchDemo()
