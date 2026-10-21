"""Load fusion workflow from a task directory or zip."""

from __future__ import annotations

import importlib.util
import shutil
import sys
import zipfile
from pathlib import Path, PurePosixPath
from tempfile import mkdtemp

from cqlib_tianyan.fusion.exceptions import PackageValidationError
from cqlib_tianyan.fusion.spec.manifest import FusionManifest, load_manifest
from cqlib_tianyan.fusion.workflow.base import FusionWorkflow

MAX_ARCHIVE_FILES = 10_000
MAX_EXTRACTED_BYTES = 100 * 1024 * 1024


def _import_module_from_path(module_name: str, file_path: Path):
    spec = importlib.util.spec_from_file_location(module_name, file_path)
    if spec is None or spec.loader is None:
        raise PackageValidationError(f"cannot import module from {file_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def load_workflow(manifest: FusionManifest, root: Path) -> FusionWorkflow:
    module_name, _, callable_name = manifest.entrypoint.partition(":")
    module_path = root / f"{module_name.replace('.', '/')}.py"
    if not module_path.is_file():
        raise PackageValidationError(f"entrypoint file not found: {module_path}")

    if str(root) not in sys.path:
        sys.path.insert(0, str(root))

    module = _import_module_from_path(module_name, module_path)
    target = getattr(module, callable_name, None)
    if target is None:
        raise PackageValidationError(
            f"callable {callable_name!r} not found in module {module_name!r}"
        )

    workflow = target() if callable(target) else target
    if not isinstance(workflow, FusionWorkflow):
        raise PackageValidationError("entrypoint must return a FusionWorkflow instance")
    workflow.max_rounds = manifest.runtime.max_rounds
    workflow._fusion_root = root  # type: ignore[attr-defined]
    return workflow


def load_workflow_from_dir(source_dir: Path | str) -> FusionWorkflow:
    root = Path(source_dir).resolve()
    manifest = load_manifest(root / "fusion.yaml")
    return load_workflow(manifest, root)


def load_workflow_from_zip(zip_path: Path | str) -> tuple[FusionManifest, FusionWorkflow, Path]:
    archive_path = Path(zip_path).resolve()
    root = Path(mkdtemp(prefix="cqfusion-"))
    try:
        with zipfile.ZipFile(archive_path) as archive:
            _safe_extract(archive, root)
        manifest = load_manifest(root / "fusion.yaml")
        workflow = load_workflow(manifest, root)
        return manifest, workflow, root
    except Exception:
        shutil.rmtree(root, ignore_errors=True)
        raise


def _safe_extract(archive: zipfile.ZipFile, root: Path) -> None:
    members = archive.infolist()
    if len(members) > MAX_ARCHIVE_FILES:
        raise PackageValidationError(
            f"archive contains too many entries ({len(members)} > {MAX_ARCHIVE_FILES})"
        )
    total_size = sum(info.file_size for info in members if not info.is_dir())
    if total_size > MAX_EXTRACTED_BYTES:
        raise PackageValidationError(
            f"archive expands beyond {MAX_EXTRACTED_BYTES} bytes ({total_size} bytes)"
        )

    root_resolved = root.resolve()
    for info in members:
        member = PurePosixPath(info.filename)
        if member.is_absolute() or ".." in member.parts:
            raise PackageValidationError(f"unsafe archive path: {info.filename!r}")
        # Unix symlinks are encoded in the upper mode bits. Do not allow an
        # archive to redirect later file reads outside the extraction root.
        if ((info.external_attr >> 16) & 0o170000) == 0o120000:
            raise PackageValidationError(f"archive symlink is not allowed: {info.filename!r}")
        target = (root / Path(*member.parts)).resolve()
        if target != root_resolved and root_resolved not in target.parents:
            raise PackageValidationError(f"unsafe archive path: {info.filename!r}")
        archive.extract(info, root)
