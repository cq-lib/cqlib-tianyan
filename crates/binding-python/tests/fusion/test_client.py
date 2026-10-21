"""Exercise the Python facade through the real Rust transport on localhost."""
import json
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from threading import Thread

import pytest

from cqlib_tianyan.fusion.client.fusion_client import FusionClient
from cqlib_tianyan.fusion.exceptions import ConfirmationRequired, CqfusionRequestError


@pytest.fixture
def api(monkeypatch):
    monkeypatch.setenv("NO_PROXY", "127.0.0.1,localhost")
    monkeypatch.setenv("no_proxy", "127.0.0.1,localhost")
    for name in ("TIANYAN_API_KEY", "CQFUSION_LOGIN_KEY", "LOGIN_KEY"):
        monkeypatch.delenv(name, raising=False)
    replies, requests = deque(), []
    class Handler(BaseHTTPRequestHandler):
        def handle_request(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            requests.append((self.command, self.path, body, self.headers))
            status, headers, data = replies.popleft() if replies else (500, {}, b"unexpected request")
            self.send_response(status)
            self.send_header("Content-Length", str(len(data)))
            for key, value in headers.items():
                self.send_header(key, value)
            self.end_headers()
            self.wfile.write(data)
        do_GET = do_POST = do_DELETE = handle_request
        def log_message(self, *args): pass
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    class API:
        host = f"http://127.0.0.1:{server.server_port}"
        calls = requests
        def reply(self, data, *, status=200, code=0):
            replies.append((status, {}, json.dumps({"code": code, "data": data, "msg": "test"}).encode()))
        def raw(self, data, headers):
            replies.append((200, headers, data))
    yield API()
    server.shutdown()
    server.server_close()
    thread.join()
    assert not replies, "configured response was never consumed"


@pytest.fixture
def client(api):
    return FusionClient(host=api.host, token="token-123", timeout=2)


@pytest.fixture
def package(tmp_path):
    path = tmp_path / "task.zip"
    path.write_bytes(b"PK")
    return path


def draft(api):
    api.reply({"package_id": "pkg-1", "package_name": "task.zip"})
    api.reply({"task_id": "207768", "status": "wait", "name": "demo"})
    api.reply({"plans": [{"id": "plan-1", "qpu": "tianyan176", "hpc": "cloud-hpc"}]})


def test_login(client, api):
    api.reply({"access_token": "abc"})
    assert client.login("login-key") == "abc"
    assert b"openId=login-key" in api.calls[0][2]


def test_submit_manual(client, api, package):
    api.reply({"package_id": "pkg-1", "package_name": "task.zip"})
    api.reply({"task_id": "207768", "status": "running", "name": "demo"})
    job = client.submit(name="demo", package=package, dispatch_mode="manual",
                        quantum_resource="tianyan176", hpc_resource="cloud-hpc")
    assert (job.id, job.status) == ("207768", "running")
    assert api.calls[-1][1].endswith("/task/run")
    assert b"PK" in api.calls[0][2]


def test_manual_submit_requires_both_resources(client, api, package):
    with pytest.raises(CqfusionRequestError, match="manual dispatch requires"):
        client.submit(name="demo", package=package, dispatch_mode="manual")
    assert not api.calls


def test_intelligent_submit_waits_for_confirmation(client, api, package):
    draft(api)
    with pytest.raises(ConfirmationRequired) as caught:
        client.submit(name="demo", package=package, quantum_resource="tianyan176")
    assert caught.value.job.id == "207768"
    assert caught.value.job.status == "awaiting_confirm"
    assert [plan.id for plan in caught.value.plans] == ["plan-1"]
    assert len(api.calls) == 3
    assert api.calls[-1][1].endswith("/task/recommend")


def test_auto_confirm_accepts_first_plan(client, api, package):
    draft(api)
    api.reply({"task_id": "207768", "status": "queued", "qpu": "tianyan176"})
    job = client.submit(name="demo", package=package, auto_confirm=True)
    assert job.status == "queued"
    assert job.quantum_resource == "tianyan176"
    assert api.calls[-1][1].endswith("/task/confirm")
    assert json.loads(api.calls[-1][2])["plan_id"] == "plan-1"


def test_wait_success(client, api):
    api.reply({"task_id": "1", "status": "running"})
    api.reply({"task_id": "1", "status": "success"})
    assert client.wait("1", poll_interval=.01, timeout=1).status == "success"


def test_api_error_preserves_fields_and_cause(client, api):
    api.reply({"task_id": "bad-id"}, status=400, code="INVALID")
    with pytest.raises(CqfusionRequestError) as caught:
        client.get("bad-id")
    error = caught.value
    assert (error.status_code, error.code, error.kind) == (400, "INVALID", "api")
    assert error.data == {"task_id": "bad-id"}
    assert error.__cause__ is not None


def test_download_sanitizes_server_filename(client, api, tmp_path):
    api.raw(b"result", {"Content-Disposition": 'attachment; filename="../../escaped.zip"'})
    files = client.download_results("task-1", tmp_path / "results")
    assert files == [tmp_path / "results" / "escaped.zip"]
    assert files[0].read_bytes() == b"result"
    assert not (tmp_path / "escaped.zip").exists()


def test_cli_submit_exits_until_confirmed(tmp_path, monkeypatch, capsys):
    from cqlib_tianyan.fusion.cli import main
    from cqlib_tianyan.fusion.client.models import FusionJob, ResourcePlan

    package = tmp_path / "task.zip"
    package.write_bytes(b"PK")

    class FakeClient:
        def __init__(self, **kwargs):
            pass

        def submit(self, **kwargs):
            raise ConfirmationRequired(
                FusionJob(id="t-9", name="demo", status="awaiting_confirm"),
                [
                    ResourcePlan(
                        id="plan-1",
                        quantum_resource="tianyan176",
                        hpc_resource="cloud-hpc",
                        rank=1,
                        score="90",
                    )
                ],
            )

    monkeypatch.setattr("cqlib_tianyan.fusion.client.fusion_client.FusionClient", FakeClient)
    code = main(["submit", str(package), "--name", "demo", "--token", "tok"])
    assert code == 2
    captured = capsys.readouterr()
    assert "awaiting_confirm" in captured.out
    assert "plan-1" in captured.out
    assert "cqfusion confirm t-9 PLAN_ID" in captured.err
