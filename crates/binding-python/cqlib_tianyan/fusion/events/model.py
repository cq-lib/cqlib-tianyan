"""Execution events aligned with fusion task trace UI."""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Literal

EventStatus = Literal[
    "pending",
    "waiting",
    "running",
    "success",
    "failed",
    "warning",
]

FLOW_NODES = [
    "submit",
    "environment",
    "orchestrator",
    "classic",
    "quantum",
    "aggregate",
    "state",
    "decision",
    "final",
    "finish",
]


@dataclass
class FusionEvent:
    node: str
    status: EventStatus
    message: str
    round: int = 0
    stage_kind: str | None = None
    stage_name: str | None = None
    timestamp: str = field(
        default_factory=lambda: datetime.now(timezone.utc).isoformat()
    )
    extra: dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


class EventEmitter:
    """Collect events in memory and optionally append to a JSONL file."""

    def __init__(self, log_path: Path | None = None):
        self.events: list[FusionEvent] = []
        self.log_path = log_path
        if log_path is not None:
            log_path.parent.mkdir(parents=True, exist_ok=True)

    def emit(
        self,
        node: str,
        status: EventStatus,
        message: str,
        *,
        round: int = 0,
        stage_kind: str | None = None,
        stage_name: str | None = None,
        **extra: Any,
    ) -> FusionEvent:
        event = FusionEvent(
            node=node,
            status=status,
            message=message,
            round=round,
            stage_kind=stage_kind,
            stage_name=stage_name,
            extra=extra,
        )
        self.events.append(event)
        if self.log_path is not None:
            with self.log_path.open("a", encoding="utf-8") as handle:
                handle.write(json.dumps(event.to_dict(), ensure_ascii=False) + "\n")
        return event
