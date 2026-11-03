"""Quantum execution through cqlib_tianyan; companion cqlib only compiles QCIS."""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from typing import Any

from .mock import QuantumJobResult


def _value(obj, name, default=None):
    value = getattr(obj, name, default)
    return value() if callable(value) else value


def _layout_map(layout) -> dict[int, int]:
    return {
        int(logical.id): int(physical.id)
        for logical, physical in layout.l2p_map.items()
    }


@dataclass
class TianYanQuantumRunner:
    platform: Any
    machine_name: str
    transpile: bool = True
    # Accepted for source compatibility; transpilation reuses ``platform``.
    login_key: str | None = field(default=None, repr=False)
    _backend: Any = field(default=None, init=False, repr=False)
    _device: Any = field(default=None, init=False, repr=False)

    def _get_backend(self):
        if self._backend is None:
            self._backend = self.platform.get_backend(self.machine_name)
        return self._backend

    def _compile_target(self):
        if self._device is None:
            backend = self._get_backend()
            device_type = getattr(backend, "device_type", None)
            if device_type is not None and str(device_type) != "superconducting":
                raise ValueError(
                    f"transpile=True needs a superconducting backend; {self.machine_name} "
                    f"is {device_type}, use transpile=False"
                )
            self._device = backend.device_config()
        return self._device

    def _compile(self, circuit: str):
        # Delayed until explicitly needed: local/mock/no-transpile never
        # imports the companion cqlib or downloads device configuration.
        try:
            from cqlib.compile import compile as compile_circuit
            from cqlib.ir import qcis
        except ImportError as exc:
            raise ImportError(
                "transpile=True requires the companion cqlib>=2.0.0b1 "
                "(pip install 'cqlib-tianyan[cqlib]'); or use transpile=False"
            ) from exc
        result = compile_circuit(qcis.loads(circuit), device=self._compile_target())
        compiled = qcis.dumps(result.circuit)
        if not compiled.strip():
            raise ValueError("transpiler did not produce QCIS text")
        metadata = result.device_metadata
        if metadata is None:
            raise ValueError("device compilation returned no layout metadata")
        return compiled, {
            "initial_layout": _layout_map(metadata.initial_layout),
            "logical_to_physical": _layout_map(metadata.final_layout),
        }

    def run(self, stage_name: str, payload: dict[str, Any]) -> QuantumJobResult:
        qcis = payload.get("qcis") or payload.get("circuit")
        circuits = [qcis] if isinstance(qcis, str) else qcis
        if (
            not isinstance(circuits, list)
            or not circuits
            or any(not isinstance(c, str) or not c.strip() for c in circuits)
        ):
            raise ValueError(
                "quantum stage payload requires nonempty QCIS text or a list of QCIS strings"
            )
        shots = int(payload.get("shots", 12000))
        timeout = float(payload.get("max_wait_time", 3600))
        interval = float(payload.get("poll_interval", 5))
        if shots <= 0 or not all(
            math.isfinite(v) and v > 0 for v in (timeout, interval)
        ):
            raise ValueError(
                "shots, max_wait_time and poll_interval must be positive and finite"
            )
        mappings = []
        if self.transpile:
            prepared = [self._compile(circuit) for circuit in circuits]
            circuits = [item[0] for item in prepared]
            mappings = [item[1] for item in prepared]
        task = self._get_backend().run(circuits, shots=shots)
        task_ids = [str(item) for item in task.task_ids]
        if not task_ids:
            raise ValueError("quantum submission returned no task IDs")
        native_results = task.wait(timeout_secs=timeout, poll_interval_secs=interval)
        results = []
        for index, item in enumerate(native_results):
            counts = _value(item, "counts")
            if not isinstance(counts, dict):
                raise ValueError("ExecutionResult.counts must be a mapping")
            result_id = _value(item, "task_id")
            if result_id is None:
                raise ValueError("ExecutionResult is missing task_id")
            results.append(
                {
                    "result_index": index,
                    "task_id": str(result_id),
                    "counts": counts,
                    "shots": _value(item, "shots", shots),
                }
            )
        if not results:
            raise ValueError("quantum wait completed without results")
        output = {
            "stage": stage_name,
            "engine": "tianyan",
            "machine": self.machine_name,
            "task_ids": task_ids,
            "results": results,
            "transpile_mappings": mappings,
        }
        if len(results) == 1:
            output["counts"] = results[0]["counts"]
        # Reject native objects/non-finite values here rather than losing a
        # completed stage when checkpoint persistence attempts JSON encoding.
        output = json.loads(json.dumps(output, allow_nan=False))
        return QuantumJobResult(query_id=task_ids[0], output=output)
