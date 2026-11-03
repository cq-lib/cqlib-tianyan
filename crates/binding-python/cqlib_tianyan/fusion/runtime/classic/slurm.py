"""Slurm-backed classic compute runner."""

from __future__ import annotations

import json
import re
import shlex
import subprocess
import sys
import textwrap
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

from cqlib_tianyan.fusion.runtime.classic.local import ClassicJobResult
from cqlib_tianyan.fusion.workflow.context import Context

if TYPE_CHECKING:
    from cqlib_tianyan.fusion.workflow.base import FusionWorkflow, StageSpec


class SlurmClassicRunner:
    """Submit classic stages through Slurm, with local dry-run fallback."""

    def __init__(
        self,
        workdir: Path,
        *,
        partition: str | None = None,
        dry_run: bool = False,
        sbatch_path: str = "sbatch",
        wait_timeout: float = 3600.0,
        poll_interval: float = 1.0,
        python_executable: str | None = None,
    ):
        self.workdir = workdir / "slurm"
        self.workdir.mkdir(parents=True, exist_ok=True)
        self.partition = partition
        self.dry_run = dry_run
        self.sbatch_path = sbatch_path
        self.wait_timeout = wait_timeout
        self.poll_interval = poll_interval
        self.python_executable = python_executable or sys.executable
        self._counter = 0

    def run(self, stage_name: str, payload: dict[str, Any]) -> ClassicJobResult:
        self._counter += 1
        job_dir = self.workdir / f"job-{self._counter:04d}"
        job_dir.mkdir(parents=True, exist_ok=True)

        input_path = job_dir / "input.json"
        output_path = job_dir / "output.json"
        script_path = job_dir / "run.sh"
        slurm_path = job_dir / "submit.slurm"

        input_path.write_text(
            json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8"
        )
        script_path.write_text(
            textwrap.dedent(
                f"""\
                #!/usr/bin/env bash
                set -euo pipefail
                {shlex.quote(self.python_executable)} - <<'PY'
                import json
                from pathlib import Path
                payload = json.loads(Path({json.dumps(str(input_path))}).read_text())
                result = {{
                    "stage": {json.dumps(stage_name)},
                    "engine": "slurm-classic",
                    "input_keys": sorted(payload.keys()),
                    "hostname": __import__("socket").gethostname(),
                }}
                Path({json.dumps(str(output_path))}).write_text(json.dumps(result))
                PY
                """
            ),
            encoding="utf-8",
        )
        script_path.chmod(0o755)

        partition_line = _partition_directive(self.partition)
        job_name = _safe_job_name(stage_name)
        slurm_path.write_text(
            textwrap.dedent(
                f"""\
                #!/bin/bash
                #SBATCH --job-name=cqfusion-{job_name}
                #SBATCH --output={shlex.quote(str(job_dir / "slurm.out"))}
                #SBATCH --error={shlex.quote(str(job_dir / "slurm.err"))}
                {partition_line}#SBATCH --time=00:10:00
                {shlex.quote(str(script_path))}
                """
            ),
            encoding="utf-8",
        )

        job_id = self._submit_and_wait(script_path, slurm_path, output_path)
        output = json.loads(output_path.read_text(encoding="utf-8"))
        return ClassicJobResult(job_id=job_id, output=output)

    def run_stage(
        self,
        workflow: FusionWorkflow,
        spec: StageSpec,
        ctx: Context,
    ) -> tuple[Context, ClassicJobResult]:
        """Execute the user stage in the Slurm job instead of the orchestrator."""
        self._counter += 1
        job_dir = self.workdir / f"job-{self._counter:04d}"
        job_dir.mkdir(parents=True, exist_ok=True)
        input_path = job_dir / "context.json"
        output_path = job_dir / "output.json"
        script_path = job_dir / "run.sh"
        slurm_path = job_dir / "submit.slurm"

        source_root = getattr(workflow, "_fusion_root", None)
        if source_root is None:
            raise ValueError(
                "Slurm execution requires a workflow loaded from a task directory"
            )
        input_path.write_text(
            json.dumps(ctx.to_dict(), ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
        script_path.write_text(
            textwrap.dedent(
                f"""\
                #!/usr/bin/env bash
                set -euo pipefail
                {shlex.quote(self.python_executable)} - <<'PY'
                import json
                from pathlib import Path
                from cqlib_tianyan.fusion.package.loader import load_workflow_from_dir
                from cqlib_tianyan.fusion.workflow.context import Context

                root = Path({json.dumps(str(source_root))})
                workflow = load_workflow_from_dir(root)
                spec = next(
                    item for item in workflow.stages()
                    if item.method_name == {json.dumps(spec.method_name)}
                )
                ctx = Context.from_dict(json.loads(Path({json.dumps(str(input_path))}).read_text()))
                next_ctx, payload = workflow.run_stage(spec, ctx)
                Path({json.dumps(str(output_path))}).write_text(
                    json.dumps({{"context": next_ctx.to_dict(), "payload": payload}}, ensure_ascii=False)
                )
                PY
                """
            ),
            encoding="utf-8",
        )
        script_path.chmod(0o755)
        partition_line = _partition_directive(self.partition)
        job_name = _safe_job_name(spec.name)
        slurm_path.write_text(
            textwrap.dedent(
                f"""\
                #!/bin/bash
                #SBATCH --job-name=cqfusion-{job_name}
                #SBATCH --output={shlex.quote(str(job_dir / "slurm.out"))}
                #SBATCH --error={shlex.quote(str(job_dir / "slurm.err"))}
                {partition_line}#SBATCH --time=01:00:00
                {shlex.quote(str(script_path))}
                """
            ),
            encoding="utf-8",
        )

        job_id = self._submit_and_wait(script_path, slurm_path, output_path)
        envelope = json.loads(output_path.read_text(encoding="utf-8"))
        next_ctx = Context.from_dict(envelope["context"])
        return next_ctx, ClassicJobResult(
            job_id=job_id, output=dict(envelope["payload"])
        )

    def _submit_and_wait(
        self,
        script_path: Path,
        slurm_path: Path,
        output_path: Path,
    ) -> str:
        if self.dry_run or not _command_exists(self.sbatch_path):
            subprocess.run(["bash", str(script_path)], check=True)
            return f"S-local-{self._counter:04d}"

        completed = subprocess.run(
            [self.sbatch_path, str(slurm_path)],
            check=True,
            capture_output=True,
            text=True,
        )
        job_id = _parse_job_id(completed.stdout) or f"S-{self._counter:04d}"
        deadline = time.monotonic() + self.wait_timeout
        while not output_path.is_file():
            if time.monotonic() >= deadline:
                raise TimeoutError(
                    f"Slurm job {job_id} did not produce output within {self.wait_timeout} seconds"
                )
            time.sleep(self.poll_interval)
        return job_id


def _command_exists(name: str) -> bool:
    from shutil import which

    return which(name) is not None


def _parse_job_id(stdout: str) -> str | None:
    match = re.search(r"Submitted batch job (\d+)", stdout)
    return match.group(1) if match else None


def _safe_job_name(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "-", value).strip("-.")
    return (cleaned or "stage")[:20]


def _partition_directive(partition: str | None) -> str:
    if not partition:
        return ""
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", partition):
        raise ValueError("invalid Slurm partition name")
    return f"#SBATCH --partition={partition}\n"
