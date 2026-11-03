"""TianYanQuantumRunner transpilation against the companion cqlib 2.x compiler."""

import builtins
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from cqlib_tianyan.fusion.runtime.quantum.tianyan import TianYanQuantumRunner

NATIVE = {"RZ", "X2P", "X2M", "Y2P", "Y2M", "CZ"}
EDGES = [(0, 1), (1, 2), (2, 3)]


def line_device():
    """Build a device the way ``TianyanBackend.device_config`` does."""
    device_mod = pytest.importorskip("cqlib.device")
    circuit_mod = pytest.importorskip("cqlib.circuit")
    qubits = [0, 1, 2, 3]
    topology = device_mod.Topology(qubits, [(a, b, f"G{a}") for a, b in EDGES])
    device = device_mod.Device("tianyan-test", qubits, topology)
    device.native_gates = [
        circuit_mod.Instruction.from_standard_gate(getattr(circuit_mod.StandardGate, g))
        for g in sorted(NATIVE)
    ]
    return device


def make_runner(device, results, device_type="superconducting"):
    task = SimpleNamespace(task_ids=["t1"], wait=Mock(return_value=results))
    backend = SimpleNamespace(
        run=Mock(return_value=task),
        device_config=Mock(return_value=device),
        device_type=device_type,
    )
    platform = SimpleNamespace(get_backend=Mock(return_value=backend))
    return TianYanQuantumRunner(platform, "tianyan-test"), backend


def test_transpile_routes_and_lowers_to_device_native_qcis():
    device = line_device()
    runner, backend = make_runner(
        device,
        [
            SimpleNamespace(task_id="t1", counts={"00": 1}, shots=1),
            SimpleNamespace(task_id="t1", counts={"11": 1}, shots=1),
        ],
    )
    source = ["H Q0\nCZ Q0 Q3\nM Q0 Q3", "X Q2\nM Q2"]
    result = runner.run("quantum", {"qcis": source, "shots": 1})

    backend.device_config.assert_called_once_with()
    submitted, kwargs = backend.run.call_args
    assert kwargs == {"shots": 1}
    compiled = submitted[0]
    assert len(compiled) == 2 and compiled != source
    edges = {frozenset(edge) for edge in EDGES}
    for text in compiled:
        for line in filter(None, (raw.split() for raw in text.splitlines())):
            gate, operands = line[0], [x for x in line[1:] if x.startswith("Q")]
            assert gate in NATIVE | {"M", "B"}, text
            if gate == "CZ":
                assert frozenset(int(q[1:]) for q in operands) in edges, text

    mappings = result.output["transpile_mappings"]
    assert len(mappings) == 2
    for mapping in mappings:
        assert set(mapping) == {"initial_layout", "logical_to_physical"}
        assert mapping["logical_to_physical"]
        assert all(0 <= p <= 3 for p in mapping["logical_to_physical"].values())


def test_transpile_rejects_non_superconducting_before_submit():
    pytest.importorskip("cqlib.compile")
    runner, backend = make_runner(None, [], device_type="simulator")
    with pytest.raises(ValueError, match="superconducting"):
        runner.run("q", {"qcis": "H Q0\nM Q0"})
    backend.device_config.assert_not_called()
    backend.run.assert_not_called()


def test_transpile_without_companion_cqlib_explains_install(monkeypatch):
    original = builtins.__import__

    def missing_cqlib(name, *args, **kwargs):
        if name == "cqlib" or name.startswith("cqlib."):
            raise ImportError(f"No module named {name!r}")
        return original(name, *args, **kwargs)

    monkeypatch.setattr(builtins, "__import__", missing_cqlib)
    runner, backend = make_runner(None, [])
    with pytest.raises(ImportError, match=r"cqlib-tianyan\[cqlib\]"):
        runner.run("q", {"qcis": "H Q0\nM Q0"})
    backend.run.assert_not_called()
