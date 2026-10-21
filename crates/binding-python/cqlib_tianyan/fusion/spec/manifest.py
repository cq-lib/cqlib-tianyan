"""fusion.yaml manifest parsing."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

from cqlib_tianyan.fusion.exceptions import ManifestError

SCHEMA_VERSION = "1.0"
VALID_MODES = {"iterative", "batch", "runtime", "generic"}


@dataclass
class ResourceHints:
    """Soft preferences for intelligent dispatch.

    These values are hints passed into recommendation. They become a binding
    only after the user confirms a plan, or when dispatch mode is manual.
    """

    quantum: str | None = None
    hpc: str | None = None


@dataclass
class ProfileHints:
    gate_set: list[str] = field(default_factory=list)
    logical_qubits: int | None = None
    estimated_physical_qubits: int | None = None


@dataclass
class RuntimeHints:
    max_rounds: int = 100
    checkpoint: bool = True


@dataclass
class FusionManifest:
    schema_version: str
    name: str
    entrypoint: str
    mode: str = "generic"
    profile: ProfileHints = field(default_factory=ProfileHints)
    resources: ResourceHints = field(default_factory=ResourceHints)
    runtime: RuntimeHints = field(default_factory=RuntimeHints)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> FusionManifest:
        if not isinstance(data, dict):
            raise ManifestError("fusion.yaml root must be a mapping")

        schema_version = str(data.get("schema_version", "")).strip()
        if schema_version != SCHEMA_VERSION:
            raise ManifestError(
                f"unsupported schema_version {schema_version!r}, expected {SCHEMA_VERSION!r}"
            )

        name = str(data.get("name", "")).strip()
        entrypoint = str(data.get("entrypoint", "")).strip()
        if not name:
            raise ManifestError("fusion.yaml requires non-empty 'name'")
        if not entrypoint or ":" not in entrypoint:
            raise ManifestError("fusion.yaml requires 'entrypoint' as module:callable")

        mode = str(data.get("mode", "generic")).strip()
        if mode not in VALID_MODES:
            raise ManifestError(f"invalid mode {mode!r}, expected one of {sorted(VALID_MODES)}")

        profile_raw = data.get("profile") or {}
        if not isinstance(profile_raw, dict):
            raise ManifestError("fusion.yaml 'profile' must be a mapping")
        gate_set_raw = profile_raw.get("gate_set", [])
        if not isinstance(gate_set_raw, list):
            raise ManifestError("fusion.yaml profile.gate_set must be a list")
        logical_qubits = _optional_positive_int(
            profile_raw.get("logical_qubits"), "profile.logical_qubits"
        )
        estimated_physical_qubits = _optional_positive_int(
            profile_raw.get("estimated_physical_qubits"),
            "profile.estimated_physical_qubits",
        )
        profile = ProfileHints(
            gate_set=[str(g).upper() for g in gate_set_raw if str(g).strip()],
            logical_qubits=logical_qubits,
            estimated_physical_qubits=estimated_physical_qubits,
        )

        resources_raw = data.get("resources") or {}
        if not isinstance(resources_raw, dict):
            raise ManifestError("fusion.yaml 'resources' must be a mapping")
        resources = ResourceHints(
            quantum=resources_raw.get("quantum"),
            hpc=resources_raw.get("hpc"),
        )

        runtime_raw = data.get("runtime") or {}
        if not isinstance(runtime_raw, dict):
            raise ManifestError("fusion.yaml 'runtime' must be a mapping")
        try:
            max_rounds = int(runtime_raw.get("max_rounds", 100))
        except (TypeError, ValueError) as exc:
            raise ManifestError("fusion.yaml runtime.max_rounds must be an integer") from exc
        if max_rounds < 1:
            raise ManifestError("fusion.yaml runtime.max_rounds must be at least 1")
        runtime = RuntimeHints(
            max_rounds=max_rounds,
            checkpoint=_as_bool(runtime_raw.get("checkpoint", True), "runtime.checkpoint"),
        )

        return cls(
            schema_version=schema_version,
            name=name,
            entrypoint=entrypoint,
            mode=mode,
            profile=profile,
            resources=resources,
            runtime=runtime,
        )


def _optional_positive_int(value: Any, field_name: str) -> int | None:
    if value is None:
        return None
    if isinstance(value, bool):
        raise ManifestError(f"fusion.yaml {field_name} must be a positive integer")
    try:
        parsed = int(value)
    except (TypeError, ValueError) as exc:
        raise ManifestError(f"fusion.yaml {field_name} must be a positive integer") from exc
    if parsed < 1:
        raise ManifestError(f"fusion.yaml {field_name} must be a positive integer")
    return parsed


def _as_bool(value: Any, field_name: str) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str) and value.strip().lower() in {"true", "false"}:
        return value.strip().lower() == "true"
    raise ManifestError(f"fusion.yaml {field_name} must be true or false")


def load_manifest(path: Path | str) -> FusionManifest:
    manifest_path = Path(path)
    if not manifest_path.is_file():
        raise ManifestError(f"manifest not found: {manifest_path}")
    with manifest_path.open(encoding="utf-8") as handle:
        data = yaml.safe_load(handle)
    return FusionManifest.from_dict(data or {})
