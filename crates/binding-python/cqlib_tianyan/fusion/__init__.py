"""cqfusion — hybrid quantum-classical fusion workflow SDK."""

from cqlib_tianyan.fusion.workflow.base import FusionWorkflow
from cqlib_tianyan.fusion.workflow.context import Context
from cqlib_tianyan.fusion.workflow.decorators import classic_stage, quantum_stage
from cqlib_tianyan.fusion.spec.profile import TaskProfile

__all__ = [
    "FusionWorkflow",
    "Context",
    "classic_stage",
    "quantum_stage",
    "TaskProfile",
]

from importlib.metadata import version, PackageNotFoundError

try:
    __version__ = version("cqlib-tianyan")
except PackageNotFoundError:
    __version__ = "0+uninstalled"
