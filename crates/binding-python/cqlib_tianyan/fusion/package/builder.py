"""Build fusion task zip packages."""

from __future__ import annotations

import zipfile
from pathlib import Path

from cqlib_tianyan.fusion.exceptions import PackageValidationError
from cqlib_tianyan.fusion.package.validator import validate_package_dir

MAX_PACKAGE_BYTES = 100 * 1024 * 1024
SKIP_DIRS = {
    ".git",
    ".venv",
    ".tox",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    "dist",
    "build",
}
SKIP_FILES = {".env", ".env.local", ".npmrc", ".pypirc"}


def build_package(source_dir: Path | str, output_zip: Path | str) -> Path:
    root = Path(source_dir).resolve()
    report = validate_package_dir(root)
    if not report.ok:
        raise PackageValidationError("; ".join(report.errors))

    out = Path(output_zip).resolve()
    out.parent.mkdir(parents=True, exist_ok=True)

    files: list[Path] = []
    source_bytes = 0
    for path in sorted(root.rglob("*")):
        if not path.is_file() or path.is_symlink() or path.resolve() == out:
            continue
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        if path.name in SKIP_FILES:
            continue
        source_bytes += path.stat().st_size
        if source_bytes > MAX_PACKAGE_BYTES:
            raise PackageValidationError(
                f"package source exceeds {MAX_PACKAGE_BYTES} bytes limit "
                f"({source_bytes} bytes)"
            )
        files.append(path)

    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in files:
            archive.write(path, arcname=path.relative_to(root).as_posix())

    size = out.stat().st_size
    if size > MAX_PACKAGE_BYTES:
        out.unlink(missing_ok=True)
        raise PackageValidationError(
            f"package exceeds {MAX_PACKAGE_BYTES} bytes limit ({size} bytes)"
        )
    return out
