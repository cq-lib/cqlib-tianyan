"""Quantum execution through cqlib_tianyan; legacy cqlib only compiles QCIS."""

from __future__ import annotations

import json
import math
from dataclasses import dataclass, field
from typing import Any

from ...config import resolve_login_key
from .mock import QuantumJobResult


def _value(obj, name, default=None):
    value = getattr(obj, name, default)
    return value() if callable(value) else value


@dataclass
class TianYanQuantumRunner:
    platform: Any
    machine_name: str
    transpile: bool = True
    login_key: str | None = field(default=None, repr=False)
    _backend: Any = field(default=None, init=False, repr=False)
    _legacy_platform: Any = field(default=None, init=False, repr=False)

    def _compile(self, circuit: str):
        # Delayed until explicitly needed: local/mock/no-transpile never
        # imports the old platform or starts its second login session.
        try:
            from cqlib import TianYanPlatform
            from cqlib.mapping import transpile_qcis
        except ImportError as exc:
            raise ImportError(
                "transpile=True requires a cqlib version providing TianYanPlatform and mapping.transpile_qcis"
            ) from exc
        if self._legacy_platform is None:
            key = resolve_login_key(self.login_key)
            if not key:
                raise ValueError(
                    "legacy transpilation needs an API key; pass login_key or use transpile=False"
                )
            self._legacy_platform = TianYanPlatform(
                login_key=key, machine_name=self.machine_name
            )
        compiled, initial, swaps, final = transpile_qcis(circuit, self._legacy_platform)
        if not isinstance(compiled, str):
            compiled = compiled.qcis()
        if not isinstance(compiled, str) or not compiled.strip():
            raise ValueError("transpiler did not produce QCIS text")
        return compiled, {
            "initial_layout": initial,
            "swap_mapping": swaps,
            "logical_to_physical": final,
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
        if self._backend is None:
            self._backend = self.platform.get_backend(self.machine_name)
        task = self._backend.run(circuits, shots=shots)
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
