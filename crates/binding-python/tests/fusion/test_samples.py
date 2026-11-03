import sys
from pathlib import Path

import pytest

from cqlib_tianyan.fusion.package.validator import validate_package_dir
from cqlib_tianyan.fusion.runtime.classic.slurm import SlurmClassicRunner
from cqlib_tianyan.fusion.runtime.factory import create_classic_runner
from cqlib_tianyan.fusion.runtime.orchestrator import Orchestrator
from cqlib_tianyan.fusion.spec.manifest import load_manifest
from cqlib_tianyan.fusion.package.loader import load_workflow_from_dir

SAMPLES = Path(__file__).resolve().parents[4] / "examples" / "fusion"


@pytest.mark.parametrize("name", ["iterative", "batch", "runtime", "legacy"])
def test_all_samples_validate(name):
    report = validate_package_dir(SAMPLES / name)
    assert report.ok, report.errors


@pytest.mark.parametrize("name", ["iterative", "batch", "runtime", "legacy"])
def test_all_samples_run_local(name, tmp_path):
    root = SAMPLES / name
    manifest = load_manifest(root / "fusion.yaml")
    workflow = load_workflow_from_dir(root)
    classic_runner = create_classic_runner("slurm", tmp_path / name, slurm_dry_run=True)
    result = Orchestrator(
        workflow,
        manifest,
        tmp_path / name / "run",
        classic_runner=classic_runner,
    ).run()
    assert result.status == "success"


def test_run_local_waits_for_confirmation(monkeypatch, tmp_path, capsys):
    from cqlib_tianyan.fusion.cli import main

    monkeypatch.setattr(sys.stdin, "isatty", lambda: False)
    code = main(
        [
            "run-local",
            str(SAMPLES / "legacy"),
            "--workdir",
            str(tmp_path / "blocked"),
            "--auto-confirm",
        ]
    )
    assert code == 0
    blocked = main(
        [
            "run-local",
            str(SAMPLES / "legacy"),
            "--workdir",
            str(tmp_path / "need-confirm"),
        ]
    )
    assert blocked == 2
    captured = capsys.readouterr()
    assert "非交互环境请加 --auto-confirm" in captured.err


def test_run_local_prompt_accepts_yes(monkeypatch, tmp_path):
    from cqlib_tianyan.fusion.cli import main

    monkeypatch.setattr(sys.stdin, "isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda prompt="": "1")
    code = main(
        ["run-local", str(SAMPLES / "legacy"), "--workdir", str(tmp_path / "yes")]
    )
    assert code == 0


def test_run_local_selects_second_plan(monkeypatch, tmp_path, capsys):
    from cqlib_tianyan.fusion.cli import main

    monkeypatch.setattr(sys.stdin, "isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda prompt="": "2")
    code = main(
        ["run-local", str(SAMPLES / "legacy"), "--workdir", str(tmp_path / "second")]
    )
    assert code == 0
    captured = capsys.readouterr()
    assert '"rank": 2' in captured.out
    assert "已选择第 2 名" in captured.err


def test_local_recommendation_prefers_hint():
    from cqlib_tianyan.fusion.runtime.recommend import recommend_local
    from cqlib_tianyan.fusion.spec.profile import TaskProfile

    plans = recommend_local(
        TaskProfile(gate_set=["CZ", "RZ", "SX"], estimated_physical_qubits=6),
        preference="balanced",
        quantum_hint="tianyan176",
        hpc_hint="cloud-hpc-batch",
    )
    assert len(plans) == 3
    assert plans[0].id == "tianyan176-cloud-hpc-batch"


def test_slurm_runner_dry_run(tmp_path):
    runner = SlurmClassicRunner(tmp_path, dry_run=True)
    result = runner.run("demo", {"x": 1})
    assert result.job_id.startswith("S-local-")
    assert result.output["engine"] == "slurm-classic"


def test_slurm_runner_waits_for_async_output(tmp_path):
    runner = SlurmClassicRunner(
        tmp_path,
        sbatch_path="/usr/bin/true",
        wait_timeout=0.01,
        poll_interval=0.001,
    )
    with pytest.raises(TimeoutError, match="did not produce output"):
        runner.run("demo", {"x": 1})
