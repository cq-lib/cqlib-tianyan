"""Compatibility imports for task packages written for cqfusion."""

import importlib as _importlib
import sys as _sys
from cqlib_tianyan.fusion import *  # noqa: F403
from cqlib_tianyan.fusion import __version__, __all__  # noqa: F401

_MODULES = [
    "config",
    "checkpoint",
    "checkpoint.store",
    "cli",
    "client",
    "client.fusion_client",
    "client.models",
    "events",
    "events.model",
    "exceptions",
    "package",
    "package.builder",
    "package.loader",
    "package.validator",
    "runtime",
    "runtime.classic",
    "runtime.classic.local",
    "runtime.classic.slurm",
    "runtime.factory",
    "runtime.orchestrator",
    "runtime.quantum",
    "runtime.quantum.mock",
    "runtime.quantum.tianyan",
    "runtime.recommend",
    "spec",
    "spec.manifest",
    "spec.profile",
    "workflow",
    "workflow.base",
    "workflow.context",
    "workflow.decorators",
]
for _name in sorted(_MODULES, key=lambda name: (name.count("."), name)):
    _module = _importlib.import_module("cqlib_tianyan.fusion." + _name)
    _sys.modules[__name__ + "." + _name] = _module
    if "." not in _name:
        globals()[_name] = _module
