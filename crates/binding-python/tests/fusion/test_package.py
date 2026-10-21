import shutil
import zipfile
from pathlib import Path

import pytest

from cqlib_tianyan.fusion import Context, FusionWorkflow, classic_stage, quantum_stage
from cqlib_tianyan.fusion.exceptions import WorkflowError
from cqlib_tianyan.fusion.package.builder import build_package
from cqlib_tianyan.fusion.package.loader import load_workflow_from_dir, load_workflow_from_zip
from cqlib_tianyan.fusion.package.validator import validate_package_dir
from cqlib_tianyan.fusion.runtime.orchestrator import Orchestrator
from cqlib_tianyan.fusion.spec.manifest import load_manifest
from cqlib_tianyan.fusion.spec.profile import extract_profile

SAMPLE = Path(__file__).resolve().parents[4] / "examples" / "fusion" / "iterative"


def test_validate_sample():
    report = validate_package_dir(SAMPLE)
    assert report.ok, report.errors
    assert report.profile is not None
    assert "CZ" in report.profile["gate_set"]


def test_load_workflow():
    workflow = load_workflow_from_dir(SAMPLE)
    assert len(workflow.classic_stages()) == 1
    assert len(workflow.quantum_stages()) == 1


def test_build_package(tmp_path):
    out = tmp_path / "task.zip"
    build_package(SAMPLE, out)
    assert out.is_file()
    assert out.stat().st_size > 0


def test_build_package_excludes_output_and_local_secrets(tmp_path):
    package = tmp_path / "package"
    shutil.copytree(SAMPLE, package)
    (package / ".env").write_text("SECRET=do-not-package", encoding="utf-8")
    out = package / "task.zip"
    build_package(package, out)
    with zipfile.ZipFile(out) as archive:
        names = archive.namelist()
    assert "task.zip" not in names
    assert ".env" not in names


def test_run_local(tmp_path):
    manifest = load_manifest(SAMPLE / "fusion.yaml")
    workflow = load_workflow_from_dir(SAMPLE)
    profile = extract_profile(manifest, workflow)
    assert profile.logical_qubits == 4

    workdir = tmp_path / "run"
    result = Orchestrator(workflow, manifest, workdir).run()
    assert result.status == "success"
    assert result.context.quantum_iterations >= 1
    assert result.context.classic_iterations >= 1
    assert len(result.subtasks) >= 2


def test_default_decision_advances_rounds_and_stops(tmp_path):
    class DefaultWorkflow(FusionWorkflow):
        max_rounds = 2

        @classic_stage()
        def first(self, ctx: Context):
            return {"classic": ctx.classic_iterations + 1}

        @quantum_stage()
        def second(self, ctx: Context):
            return {"shots": 4}

    manifest = load_manifest(SAMPLE / "fusion.yaml")
    workflow = DefaultWorkflow()
    workflow.max_rounds = 2
    result = Orchestrator(workflow, manifest, tmp_path / "default").run()
    assert result.context.round == 1
    assert result.context.classic_iterations == 2
    assert result.context.quantum_iterations == 2


def test_quantum_only_workflow_can_start(tmp_path):
    class QuantumOnly(FusionWorkflow):
        max_rounds = 1

        @quantum_stage()
        def execute(self, ctx: Context):
            return {"shots": 2}

    manifest = load_manifest(SAMPLE / "fusion.yaml")
    result = Orchestrator(QuantumOnly(), manifest, tmp_path / "quantum-only").run()
    assert result.context.classic_iterations == 0
    assert result.context.quantum_iterations == 1


def test_stage_order_follows_definition_order():
    class Ordered(FusionWorkflow):
        @classic_stage()
        def z_first(self, ctx: Context):
            return {}

        @classic_stage()
        def a_second(self, ctx: Context):
            return {}

    assert [stage.method_name for stage in Ordered().stages()] == ["z_first", "a_second"]


def test_zip_loader_keeps_extracted_root_alive(tmp_path):
    archive = build_package(SAMPLE, tmp_path / "task.zip")
    _, _, root = load_workflow_from_zip(archive)
    try:
        assert root.is_dir()
        assert (root / "fusion.yaml").is_file()
    finally:
        shutil.rmtree(root)


def test_validation_does_not_execute_entrypoint(tmp_path):
    marker = tmp_path / "executed"
    package = tmp_path / "package"
    package.mkdir()
    (package / "fusion.yaml").write_text(
        "schema_version: '1.0'\nname: safe-check\nentrypoint: main:app\n",
        encoding="utf-8",
    )
    (package / "main.py").write_text(
        "from pathlib import Path\n"
        f"Path({str(marker)!r}).write_text('executed')\n"
        "from cqlib_tianyan.fusion import FusionWorkflow, classic_stage\n"
        "class Demo(FusionWorkflow):\n"
        "    @classic_stage()\n"
        "    def run(self, ctx): return {}\n"
        "app = Demo()\n",
        encoding="utf-8",
    )
    report = validate_package_dir(package)
    assert report.ok, report.errors
    assert not marker.exists()


def test_checkpoint_resume_does_not_repeat_completed_stage(tmp_path):
    class FailingQuantumRunner:
        def run(self, stage_name, payload):
            raise RuntimeError("temporary quantum failure")

    manifest = load_manifest(SAMPLE / "fusion.yaml")
    workdir = tmp_path / "resume"
    with pytest.raises(WorkflowError):
        Orchestrator(
            load_workflow_from_dir(SAMPLE),
            manifest,
            workdir,
            quantum_runner=FailingQuantumRunner(),
        ).run()

    result = Orchestrator(load_workflow_from_dir(SAMPLE), manifest, workdir).run()
    assert result.context.classic_iterations == 3
    assert result.context.quantum_iterations == 3
