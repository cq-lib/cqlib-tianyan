"""Exercise the Python facade through the real Rust transport on localhost."""
import json
import zipfile
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
        def reply(self, data, *, status=200, code=200):
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
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("main.py", "print(1)")
        archive.writestr("circuit.qcis", "H Q0\nM Q0")
    return path


def draft(api):
    api.reply({"fileId": "pkg-1"})
    api.raw(json.dumps({"recommended_group_id":"plan-1", "ranked_candidates":[{
        "rank":1, "group_id":"plan-1", "qpu":{"device_id":"tianyan176"},
        "classic":{"resource_id":"cloud-hpc"}, "meets_all_constraints":True
    }]}).encode(), {"Content-Type":"application/json"})
    choices(api)


def choices(api):
    api.reply({"qpuTotal":1, "cpuTotal":1,
               "qpu":[{"code":"tianyan176", "available":True}],
               "cpu":[{"code":"cloud-hpc", "available":True}]})


def test_login(client, api):
    api.reply({"access_token": "abc"}, code=0)
    assert client.login("login-key") == "abc"
    assert b"openId=login-key" in api.calls[0][2]


def test_submit_manual(client, api, package):
    api.reply({"fileId": "pkg-1"})
    choices(api)
    api.reply({"id": "207768", "status": 8, "taskName": "demo"})
    job = client.submit_job(name="demo", package=package, dispatch_mode="manual",
                        quantum_resource="tianyan176", hpc_resource="cloud-hpc")
    assert (job.id, job.status) == ("207768", "running")
    assert api.calls[-1][1].endswith("/hybrid/job/v2/submit")
    assert b"PK" in api.calls[0][2]


def test_manual_submit_requires_both_resources(client, api, package):
    with pytest.raises(CqfusionRequestError, match="manual dispatch requires"):
        client.submit_job(name="demo", package=package, dispatch_mode="manual")
    assert not api.calls


def test_intelligent_submit_waits_for_confirmation(client, api, package):
    draft(api)
    with pytest.raises(ConfirmationRequired) as caught:
        client.submit_job(name="demo", package=package, quantum_resource="tianyan176")
    assert caught.value.job.id == ""
    assert caught.value.job.raw["fileId"] == "pkg-1"
    assert caught.value.job.status == "awaiting_confirm"
    assert [plan.id for plan in caught.value.plans] == ["plan-1"]
    assert len(api.calls) == 3
    assert api.calls[-1][1].endswith("/resources/choices?pageNo=1&pageSize=100")


def test_auto_confirm_accepts_first_plan(client, api, package):
    draft(api)
    api.reply({"id": "207768", "status": 2, "qpuResourceCode": "tianyan176"})
    job = client.submit_job(name="demo", package=package, auto_confirm=True)
    assert job.status == "queued"
    assert job.quantum_resource == "tianyan176"
    assert api.calls[-1][1].endswith("/hybrid/job/v2/submit")
    assert json.loads(api.calls[-1][2])["qpuResourceCode"] == "tianyan176"


def test_wait_success(client, api):
    api.reply({"status": 8})
    api.reply({"status": 6})
    job = client.wait_job("1", poll_interval=.01, timeout=1)
    assert (job.id, job.status) == ("1", "success")


def test_api_error_preserves_fields_and_cause(client, api):
    api.reply({"task_id": "bad-id"}, status=400, code="INVALID")
    with pytest.raises(CqfusionRequestError) as caught:
        client.get_job("bad-id")
    error = caught.value
    assert (error.status_code, error.code, error.kind) == (400, "INVALID", "api")
    assert error.data == {"task_id": "bad-id"}
    assert error.__cause__ is not None


def test_download_sanitizes_server_filename(client, api, tmp_path):
    api.raw(b"result", {"Content-Disposition": 'attachment; filename="../../escaped.zip"'})
    files = [client.download_job_result("task-1", tmp_path / "results")]
    assert files == [tmp_path / "results" / "escaped.zip"]
    assert files[0].read_bytes() == b"result"
    assert not (tmp_path / "escaped.zip").exists()


def test_cli_submit_exits_until_confirmed(tmp_path, monkeypatch, capsys):
    from cqlib_tianyan.fusion.cli import main
    package = tmp_path / "task.zip"
    package.write_bytes(b"PK")

    class Schedule:
        def candidates(self):
            return [{"group_id":"plan-1", "rank":1, "selectable":True}]

    class FakeClient:
        def __init__(self, **kwargs): pass
        def schedule_package(self, *args): return Schedule()
        def submit_decision(self, *args):
            pytest.fail("must not submit before selection")

    monkeypatch.setattr("cqlib_tianyan.fusion.client.fusion_client.FusionClient", FakeClient)
    code = main(["submit", str(package), "--name", "demo", "--token", "tok"])
    assert code == 2
    captured = capsys.readouterr()
    assert "plan-1" in captured.out
    assert "No job created" in captured.err


def test_v2_schedule_decision_wait_and_json_result(client, api, package, tmp_path):
    draft(api)
    api.reply({"id":"88", "status":2})
    api.reply({"id":"88", "status":6})
    api.raw(b'{"result":"success"}', {"Content-Type":"application/json"})
    schedule = client.schedule_package(package)
    assert schedule.candidates()[0]["selectable"]
    job = client.submit_decision("demo", schedule.select(1))
    assert job.task_id() == "88"
    assert json.loads(job.wait(timeout=1, poll_interval=.01))["status"] == 6
    assert json.loads(__import__("pathlib").Path(job.download(str(tmp_path))).read_text())["result"] == "success"
    assert api.calls[-2][1].endswith("/tasks/88/progress")
    assert all("/sdk/fusion" not in call[1] for call in api.calls)


def test_v2_common_apis(client, api, tmp_path):
    api.reply({"total":0, "pageNo":2, "pageSize":10, "records":[]})
    api.reply([{"presetId":"iterative", "fileId":"public-1"}])
    api.reply({"id":"88", "status":8})
    api.raw(b"done", {"Content-Type":"text/plain"})
    api.reply({"id":"88", "deleted":True})
    assert client.list_jobs(page_no=2, page_size=10, view="DRAFT")["total"] == 0
    assert client.presets()[0]["fileId"] == "public-1"
    assert client.progress("88")["status"] == 8
    assert client.download_job_log("88", tmp_path).read_text() == "done"
    assert client.delete_job("88")["deleted"]
    assert api.calls[0][1].endswith("/tasks?pageNo=2&pageSize=10&view=DRAFT")
    assert api.calls[-1][:2] == ("DELETE", "/qccp-hybrid/hybrid/job/v2/tasks/88")


@pytest.mark.parametrize("method,args", [("confirm", ("88", "plan")), ("recommend", ("88",)),
                                          ("events", ("88",)), ("subtasks", ("88",)),
                                          ("list_tasks", ())])
def test_legacy_calls_warn(client, api, method, args):
    api.reply({"id":"88"} if method == "confirm" else [], code=0)
    with pytest.warns(DeprecationWarning, match="not deployed"):
        getattr(client, method)(*args)
    assert "/sdk/fusion" in api.calls[0][1]


@pytest.mark.parametrize("entries,message", [
    ({"nested/main.py":"", "a.qcis":"M Q0"}, "ZIP root"),
    ({"main.py":""}, "exactly one .qcis"),
    ({"main.py":"", "a.qcis":"M Q0", "b.qcis":"M Q0"}, "found 2"),
    ({"main.py":"", "a.qcis":"M Q0", "tianyan.env":"TIANYAN_APP_SHOTS=0"}, "TIANYAN_APP_SHOTS"),
    ({"main.py":"", "a.qcis":"M Q0", "tianyan.env":"TIANYAN_APP_SHOTS=2\nTIANYAN_APP_SHOTS=3"}, "exactly once"),
])
def test_invalid_archive_never_uploads(client, api, tmp_path, entries, message):
    package = tmp_path / "invalid.zip"
    with zipfile.ZipFile(package, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in entries.items(): archive.writestr(name, content)
    with pytest.raises(CqfusionRequestError, match=message):
        client.upload_job_package(package)
    assert not api.calls


def test_v2_base_info_does_not_guess_runtime_status_or_counts():
    from cqlib_tianyan.fusion.client.models import FusionJob
    job = FusionJob.from_api({"id":"88", "taskName":"demo", "qpuResourceCode":"q"})
    assert job.name == "demo"
    assert job.status == "unknown"
    assert job.classic_iterations is None
    assert job.quantum_iterations is None


@pytest.mark.parametrize("args", [
    ["save", "missing.zip", "--name", "demo"],
    ["recommend", "88"],
    ["confirm", "88", "plan"],
])
def test_deprecated_cli_commands_exit_without_requests(args, monkeypatch, capsys):
    from cqlib_tianyan.fusion import cli

    def unexpected(*args, **kwargs):
        pytest.fail("deprecated command must not create a client or prepare a package")

    monkeypatch.setattr(cli, "_make_client", unexpected)
    monkeypatch.setattr(cli, "_prepare_package", unexpected)
    assert cli.main(args) != 0
    captured = capsys.readouterr()
    assert not captured.out
    assert "deprecated" in captured.err
    assert "not deployed" in captured.err


@pytest.mark.parametrize("method,args,kwargs", [
    ("upload_package", ("/nonexistent/fusion-task.zip",), {}),
    ("submit", (), {"name": "demo", "package": "/nonexistent/fusion-task.zip", "dispatch_mode": "manual"}),
    ("get", ("",), {}),
    ("delete", ("",), {}),
    ("wait", ("",), {"timeout": -1}),
    ("download_results", ("", "/nonexistent/fusion-results"), {}),
])
def test_original_api_names_warn_before_local_validation(method, args, kwargs):
    client = FusionClient(host="http://127.0.0.1:1", token="test-token", login_key="test-key", timeout=1)
    with pytest.warns(DeprecationWarning, match="not deployed"):
        with pytest.raises(CqfusionRequestError):
            getattr(client, method)(*args, **kwargs)
