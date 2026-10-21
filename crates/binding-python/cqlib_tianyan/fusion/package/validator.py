"""Validate fusion task directories."""

from __future__ import annotations

import ast
from dataclasses import dataclass, field
from pathlib import Path

from cqlib_tianyan.fusion.spec.manifest import load_manifest
from cqlib_tianyan.fusion.spec.profile import extract_profile


@dataclass
class ValidationReport:
    ok: bool
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    profile: dict | None = None

    def __str__(self) -> str:
        if self.ok:
            return "validation passed"
        return "validation failed: " + "; ".join(self.errors)


def validate_package_dir(source_dir: Path | str) -> ValidationReport:
    root = Path(source_dir).resolve()
    errors: list[str] = []
    warnings: list[str] = []

    manifest_path = root / "fusion.yaml"
    if not manifest_path.is_file():
        return ValidationReport(ok=False, errors=["missing fusion.yaml"])

    try:
        manifest = load_manifest(manifest_path)
    except Exception as exc:  # noqa: BLE001 - surface manifest issues to caller
        return ValidationReport(ok=False, errors=[str(exc)])

    module_name, _, callable_name = manifest.entrypoint.partition(":")
    if not module_name or not callable_name:
        errors.append("invalid entrypoint, expected module:callable")

    entry_file = root / f"{module_name.replace('.', '/')}.py"
    if not entry_file.is_file():
        errors.append(f"entrypoint module file not found: {entry_file.name}")

    profile_dict = None
    if not errors:
        try:
            tree = ast.parse(entry_file.read_text(encoding="utf-8"), filename=str(entry_file))
            defined_names = _top_level_defined_names(tree)
            if callable_name not in defined_names:
                errors.append(
                    f"entrypoint callable {callable_name!r} is not defined in {entry_file.name}"
                )
            python_trees = [tree]
            for source in root.rglob("*.py"):
                if source == entry_file or source.is_symlink():
                    continue
                python_trees.append(
                    ast.parse(source.read_text(encoding="utf-8"), filename=str(source))
                )
            if not any(_has_stage_decorator(item) for item in python_trees):
                errors.append("workflow defines no classic/quantum stages")
        except (OSError, UnicodeError, SyntaxError) as exc:
            errors.append(f"failed to statically inspect workflow: {exc}")

    if not errors:
        # Validation runs at the upload trust boundary and must never import the
        # submitted module. Decorator metadata is merged later inside the
        # isolated runtime when the workflow is deliberately executed.
        profile = extract_profile(manifest)
        profile_dict = profile.to_dict()
        if not profile.gate_set and manifest.mode in {"iterative", "batch"}:
            warnings.append("task profile has empty gate_set")

    return ValidationReport(
        ok=not errors,
        errors=errors,
        warnings=warnings,
        profile=profile_dict,
    )


def _top_level_defined_names(tree: ast.Module) -> set[str]:
    names: set[str] = set()
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            names.add(node.name)
        elif isinstance(node, (ast.Assign, ast.AnnAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            for target in targets:
                if isinstance(target, ast.Name):
                    names.add(target.id)
        elif isinstance(node, ast.Import):
            for alias in node.names:
                names.add(alias.asname or alias.name.partition(".")[0])
        elif isinstance(node, ast.ImportFrom):
            for alias in node.names:
                names.add(alias.asname or alias.name)
    return names


def _has_stage_decorator(tree: ast.Module) -> bool:
    for node in ast.walk(tree):
        if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            continue
        for decorator in node.decorator_list:
            target = decorator.func if isinstance(decorator, ast.Call) else decorator
            if isinstance(target, ast.Name) and target.id in {"classic_stage", "quantum_stage"}:
                return True
            if isinstance(target, ast.Attribute) and target.attr in {
                "classic_stage",
                "quantum_stage",
            }:
                return True
    return False
