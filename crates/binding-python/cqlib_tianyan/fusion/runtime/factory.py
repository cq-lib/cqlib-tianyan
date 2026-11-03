"""Runner factory for local/cloud execution."""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Literal

from cqlib_tianyan.fusion.runtime.classic.local import LocalClassicRunner
from cqlib_tianyan.fusion.runtime.classic.slurm import SlurmClassicRunner
from cqlib_tianyan.fusion.runtime.quantum.mock import MockQuantumRunner
from cqlib_tianyan.fusion.spec.manifest import FusionManifest

ClassicBackend = Literal["local", "slurm"]
QuantumBackend = Literal["mock", "tianyan"]


def create_classic_runner(
    backend: ClassicBackend,
    workdir: Path,
    *,
    slurm_partition: str | None = None,
    slurm_dry_run: bool = False,
) -> LocalClassicRunner | SlurmClassicRunner:
    if backend == "slurm":
        return SlurmClassicRunner(
            workdir,
            partition=slurm_partition,
            dry_run=slurm_dry_run,
        )
    return LocalClassicRunner()


def create_quantum_runner(
    backend: QuantumBackend,
    manifest: FusionManifest,
    *,
    machine_name: str | None = None,
    login_key: str | None = None,
    transpile: bool = True,
    platform: Any = None,
) -> Any:
    if backend == "tianyan":
        from cqlib_tianyan import TianyanPlatform
        from cqlib_tianyan.fusion.config import resolve_login_key
        from cqlib_tianyan.fusion.runtime.quantum.tianyan import TianYanQuantumRunner

        key = resolve_login_key(login_key)
        machine = (
            machine_name
            or manifest.resources.quantum
            or os.environ.get("CQFUSION_MACHINE")
        )
        if not machine:
            raise ValueError(
                "specify a backend code with --machine, manifest resources.quantum or CQFUSION_MACHINE"
            )
        if platform is None:
            if not key:
                raise ValueError(
                    "set TIANYAN_API_KEY, CQFUSION_LOGIN_KEY or LOGIN_KEY for tianyan backend"
                )
            platform = TianyanPlatform.login(key)
        return TianYanQuantumRunner(
            platform=platform, machine_name=machine, transpile=transpile, login_key=key
        )
    return MockQuantumRunner()
