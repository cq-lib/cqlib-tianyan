"""Python facade for the Rust fusion SDK client (no Python HTTP transport)."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Literal

from ..config import resolve_login_key
from ..exceptions import ConfirmationRequired, CqfusionRequestError
from .models import FusionJob, FusionSubTask, ResourcePlan

DispatchMode = Literal["intelligent", "manual"]
DispatchPreference = Literal["quality", "speed", "cost", "balanced"]
DEFAULT_HOST = "https://qc.zdxlz.com"


def _native_call(call, *args):
    from cqlib_tianyan._cqlib_tianyan import FusionNativeError
    try:
        return call(*args)
    except FusionNativeError as exc:
        try:
            detail = json.loads(str(exc))
        except (ValueError, TypeError):
            detail = {"message": str(exc)}
        error = CqfusionRequestError(detail.get("message", str(exc)), status_code=detail.get("http_status"))
        error.code = detail.get("code")
        error.data = detail.get("data")
        error.kind = detail.get("kind")
        raise error from exc


class FusionClient:
    """Fusion cloud API; pass platform= to share quantum authentication."""
    def __init__(self, host: str = DEFAULT_HOST, token: str = "", login_key: str | None = None,
                 timeout: float = 120, *, platform: Any = None):
        from cqlib_tianyan._cqlib_tianyan import _FusionClient
        self.host = host.rstrip("/")
        self.login_key = resolve_login_key(login_key)
        self.timeout = timeout
        self._native = None
        if platform is not None:
            if token or login_key or host != DEFAULT_HOST:
                raise CqfusionRequestError("platform supplies authentication and host; do not also pass credentials or host")
            self._native = _native_call(_FusionClient.from_platform, platform, timeout)
        elif token or self.login_key:
            self._native = _native_call(_FusionClient, self.host, token, self.login_key, timeout)

    def _client(self):
        if self._native is None:
            raise CqfusionRequestError("set TIANYAN_API_KEY, CQFUSION_LOGIN_KEY or LOGIN_KEY, or pass token/platform")
        return self._native

    def _json(self, method: str, *args):
        return json.loads(_native_call(getattr(self._client(), method), *args))

    def login(self, login_key: str | None = None) -> str:
        key = resolve_login_key(login_key or self.login_key)
        if not key:
            raise CqfusionRequestError("login_key is required")
        self.login_key = key
        if self._native is None:
            from cqlib_tianyan._cqlib_tianyan import _FusionClient
            self._native = _native_call(_FusionClient, self.host, "", key, self.timeout)
            return _native_call(self._native.access_token)
        return _native_call(self._native.login, key)

    def schedule_package(self, package: Path | str, strategy: str = "balanced"):
        """Upload and assess a ZIP without creating a job; select or manually bind resources."""
        return _native_call(self._client().schedule_package, str(package), strategy)

    def submit_decision(self, name: str, decision):
        """Submit a confirmed v2 decision; returns the native job handle."""
        return _native_call(self._client().submit_decision, name, decision)

    def list_jobs(self, *, page_no: int = 1, page_size: int = 20, view: str = "ALL") -> dict[str, Any]:
        return self._json("list_jobs", page_no, page_size, view)

    def presets(self) -> list[dict[str, Any]]:
        return self._json("presets")

    def progress(self, task_id: str) -> dict[str, Any]:
        return self._json("progress", str(task_id))

    def delete_job(self, task_id: str) -> dict[str, Any]:
        return self._json("delete_job", str(task_id))

    def download_job_log(self, task_id: str, dest_dir: Path | str) -> Path:
        return Path(_native_call(self._client().download_job_log, str(task_id), str(dest_dir)))

    def upload_job_package(self, package: Path | str) -> dict[str, Any]:
        return self._json("upload_job_package", str(package))

    def get_job(self, task_id: str) -> FusionJob:
        return FusionJob.from_api(self._json("get_job", str(task_id)))

    def wait_job(self, task_id: str, *, poll_interval: float = 5.0, timeout: float = 3600.0) -> FusionJob:
        return FusionJob.from_api(self._json("wait_job", str(task_id), timeout, poll_interval))

    def download_job_result(self, task_id: str, dest_dir: Path | str) -> Path:
        return Path(_native_call(self._client().download_job_result, str(task_id), str(dest_dir)))

    def submit_job(self, *, name: str, package: Path | str, dispatch_mode: DispatchMode = "intelligent",
                   dispatch_preference: DispatchPreference = "balanced", quantum_resource: str | None = None,
                   hpc_resource: str | None = None, auto_confirm: bool = False, plan_id: str | None = None) -> FusionJob:
        options = dict(name=name, package=str(package), dispatch_mode=dispatch_mode,
                       dispatch_preference=dispatch_preference, quantum_resource=quantum_resource,
                       hpc_resource=hpc_resource, auto_confirm=auto_confirm, plan_id=plan_id)
        outcome = self._json("submit_job", json.dumps(options))
        job = FusionJob.from_api(outcome["job"])
        if outcome["awaiting_confirmation"]:
            raise ConfirmationRequired(job, [ResourcePlan.from_api(p, rank=i) for i, p in enumerate(outcome["plans"], 1)])
        return job

    def upload_package(self, package: Path | str) -> dict[str, Any]:
        return self._json("upload_package", str(package))

    # Deprecated /sdk/fusion APIs are retained for compatibility, but are not deployed.
    def save(self, *, name: str, package: Path | str, dispatch_mode: DispatchMode = "intelligent",
             dispatch_preference: DispatchPreference = "balanced", quantum_resource: str | None = None,
             hpc_resource: str | None = None) -> FusionJob:
        options = dict(name=name, package=str(package), dispatch_mode=dispatch_mode,
                       dispatch_preference=dispatch_preference, quantum_resource=quantum_resource, hpc_resource=hpc_resource)
        return FusionJob.from_api(self._json("save", json.dumps(options)))

    def recommend(self, task_id: str, *, preference: DispatchPreference = "balanced",
                  quantum_hint: str | None = None, hpc_hint: str | None = None) -> list[ResourcePlan]:
        rows = self._json("recommend", str(task_id), preference, quantum_hint, hpc_hint)
        return [ResourcePlan.from_api(row, rank=i) for i, row in enumerate(rows, 1)]

    def confirm(self, task_id: str, plan_id: str) -> FusionJob:
        return FusionJob.from_api(self._json("confirm", str(task_id), str(plan_id)))

    def submit(self, *, name: str, package: Path | str, dispatch_mode: DispatchMode = "intelligent",
               dispatch_preference: DispatchPreference = "balanced", quantum_resource: str | None = None,
               hpc_resource: str | None = None, auto_confirm: bool = False, plan_id: str | None = None) -> FusionJob:
        options = dict(name=name, package=str(package), dispatch_mode=dispatch_mode,
                       dispatch_preference=dispatch_preference, quantum_resource=quantum_resource,
                       hpc_resource=hpc_resource, auto_confirm=auto_confirm, plan_id=plan_id)
        outcome = self._json("submit", json.dumps(options))
        job = FusionJob.from_api(outcome["job"])
        if outcome["awaiting_confirmation"]:
            raise ConfirmationRequired(job, [ResourcePlan.from_api(p, rank=i) for i, p in enumerate(outcome["plans"], 1)])
        return job

    def list_tasks(self, *, name: str | None = None, status: str | None = None,
                   dispatch_mode: str | None = None, limit: int = 20, offset: int = 0) -> list[FusionJob]:
        if limit <= 0 or offset < 0:
            raise CqfusionRequestError("limit must be positive and offset nonnegative")
        query = [["limit", str(limit)], ["offset", str(offset)]]
        query += [[key, value] for key, value in (("name", name), ("status", status), ("dispatch_mode", dispatch_mode)) if value]
        return [FusionJob.from_api(row) for row in self._json("list_tasks", json.dumps(query))]

    def get(self, task_id: str) -> FusionJob:
        return FusionJob.from_api(self._json("get", str(task_id)))

    def events(self, task_id: str) -> list[dict[str, Any]]:
        return self._json("events", str(task_id))

    def subtasks(self, task_id: str) -> list[FusionSubTask]:
        return [FusionSubTask.from_api(row) for row in self._json("subtasks", str(task_id))]

    def download_results(self, task_id: str, dest_dir: Path | str) -> list[Path]:
        return [Path(_native_call(self._client().download_results, str(task_id), str(dest_dir)))]

    def wait(self, task_id: str, *, poll_interval: float = 5.0, timeout: float = 3600.0) -> FusionJob:
        return FusionJob.from_api(self._json("wait", str(task_id), timeout, poll_interval))

    def delete(self, task_id: str) -> None:
        _native_call(self._client().delete, str(task_id))

    create = save
    run = submit
