"""cqfusion exceptions."""


class CqfusionError(Exception):
    """Base error."""


class PackageValidationError(CqfusionError):
    """Task package failed validation."""


class WorkflowError(CqfusionError):
    """Workflow execution failed."""


class ManifestError(CqfusionError):
    """Invalid fusion.yaml."""


class CqfusionRequestError(CqfusionError):
    """HTTP/API request failed."""

    def __init__(self, message: str, status_code: int | None = None):
        super().__init__(message)
        self.status_code = status_code
        self.code = None
        self.data = None
        self.kind = None


class ConfirmationRequired(CqfusionError):
    """Intelligent dispatch saved a draft and is waiting for a resource plan."""

    def __init__(self, job: object, plans: list[object]):
        self.job = job
        self.plans = plans
        task_id = getattr(job, "id", "")
        super().__init__(
            f"task {task_id} is awaiting resource confirmation; "
            "choose a plan, or pass --auto-confirm to accept the first recommendation"
        )
