"""Checkpoint persistence between workflow rounds."""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from cqlib_tianyan.fusion.workflow.context import Context


class CheckpointStore:
    def __init__(self, directory: Path):
        self.directory = directory
        self.directory.mkdir(parents=True, exist_ok=True)
        self.path = self.directory / "context.json"

    def save(self, ctx: Context) -> None:
        self.save_state({"context": ctx.to_dict()})

    def load(self) -> Context | None:
        state = self.load_state()
        if state is None:
            return None
        payload = state.get("context", state)
        return Context.from_dict(payload)

    def save_state(self, state: dict[str, Any]) -> None:
        temporary = self.path.with_suffix(".json.tmp")
        with temporary.open("w", encoding="utf-8") as handle:
            json.dump(state, handle, ensure_ascii=False, indent=2)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, self.path)

    def load_state(self) -> dict[str, Any] | None:
        if not self.path.is_file():
            return None
        with self.path.open(encoding="utf-8") as handle:
            payload = json.load(handle)
        if not isinstance(payload, dict):
            raise ValueError("checkpoint root must be an object")
        return payload

    def clear(self) -> None:
        self.path.unlink(missing_ok=True)
