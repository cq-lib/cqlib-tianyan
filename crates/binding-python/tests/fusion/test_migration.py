import importlib
import json
import shutil
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from cqlib_tianyan.fusion.config import resolve_login_key
from cqlib_tianyan.fusion.runtime.quantum.tianyan import TianYanQuantumRunner


def test_old_imports_share_class_identity():
    import cqfusion
    import cqlib_tianyan.fusion as fusion
    for name in fusion.__all__:
        assert getattr(cqfusion, name) is getattr(fusion, name)
    for suffix in ('workflow.base', 'workflow.context', 'workflow.decorators',
                   'exceptions', 'runtime.classic.slurm', 'client.fusion_client', 'config'):
        assert importlib.import_module('cqfusion.' + suffix) is importlib.import_module('cqlib_tianyan.fusion.' + suffix)


def test_old_task_package_runs_in_slurm_subprocess(tmp_path):
    from cqlib_tianyan.fusion.package.loader import load_workflow_from_dir
    from cqlib_tianyan.fusion.runtime.orchestrator import Orchestrator
    from cqlib_tianyan.fusion.runtime.classic.slurm import SlurmClassicRunner
    from cqlib_tianyan.fusion.spec.manifest import load_manifest
    source = Path(__file__).resolve().parents[4] / 'examples/fusion/iterative'
    package = tmp_path / 'old-task'
    shutil.copytree(source, package)
    for path in package.rglob('*.py'):
        path.write_text(path.read_text().replace('cqlib_tianyan.fusion', 'cqfusion'))
    workflow = load_workflow_from_dir(package)
    result = Orchestrator(workflow, load_manifest(package / 'fusion.yaml'), tmp_path / 'run',
                          classic_runner=SlurmClassicRunner(tmp_path, dry_run=True)).run()
    assert result.status == 'success'


def test_login_key_precedence(monkeypatch):
    for key in ('TIANYAN_API_KEY', 'CQFUSION_LOGIN_KEY', 'LOGIN_KEY'):
        monkeypatch.delenv(key, raising=False)
    assert resolve_login_key() is None
    monkeypatch.setenv('LOGIN_KEY', 'old')
    assert resolve_login_key() == 'old'
    monkeypatch.setenv('CQFUSION_LOGIN_KEY', 'compat')
    assert resolve_login_key() == 'compat'
    monkeypatch.setenv('TIANYAN_API_KEY', 'new')
    assert resolve_login_key() == 'new'
    assert resolve_login_key('explicit') == 'explicit'


def runner(results):
    task = SimpleNamespace(task_ids=['t1', 't2'], wait=Mock(return_value=results))
    backend = SimpleNamespace(run=Mock(return_value=task))
    platform = SimpleNamespace(get_backend=Mock(return_value=backend))
    return TianYanQuantumRunner(platform, 'machine', transpile=False), task, backend


def test_batch_quantum_preserves_all_results_without_legacy_import(monkeypatch):
    import builtins
    original = builtins.__import__
    def guarded(name, *args, **kwargs):
        assert name != 'cqlib' and not name.startswith('cqlib.'), 'legacy cqlib used without transpilation'
        return original(name, *args, **kwargs)
    monkeypatch.setattr(builtins, '__import__', guarded)
    r, task, backend = runner([SimpleNamespace(task_id='t1', counts={'0': 2}, shots=2),
                               SimpleNamespace(task_id='t2', counts={'1': 2}, shots=2)])
    result = r.run('quantum', {'qcis': ['H Q1', 'X Q1'], 'shots': 2})
    backend.run.assert_called_once_with(['H Q1', 'X Q1'], shots=2)
    task.wait.assert_called_once_with(timeout_secs=3600., poll_interval_secs=5.)
    assert result.output['task_ids'] == ['t1', 't2']
    assert [item['counts'] for item in result.output['results']] == [{'0': 2}, {'1': 2}]
    assert 'counts' not in result.output
    json.dumps(result.output, allow_nan=False)


def test_single_result_keeps_counts():
    r, _, _ = runner([SimpleNamespace(task_id='t1', counts={'00': 4}, shots=4)])
    assert r.run('q', {'qcis': 'M Q1'}).output['counts'] == {'00': 4}


def test_quantum_failure_does_not_become_success():
    r, task, _ = runner([])
    task.wait.side_effect = TimeoutError('still running')
    with pytest.raises(TimeoutError):
        r.run('q', {'qcis': 'M Q1'})


def test_invalid_quantum_payload_never_submits():
    r, _, backend = runner([])
    with pytest.raises(ValueError):
        r.run('q', {'qcis': [''], 'shots': 0})
    backend.run.assert_not_called()
