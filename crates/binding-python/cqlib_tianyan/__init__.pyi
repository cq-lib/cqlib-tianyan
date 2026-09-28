# This code is part of Cqlib.
#
# (C) Copyright China Telecom Quantum Group 2026
#
# This code is licensed under the Apache License, Version 2.0. You may
# obtain a copy of this license in the LICENSE.txt file in the root directory
# of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
#
# Any modifications or derivative works of this code must retain this
# copyright notice, and modified files need to carry a notice indicating
# that they have been altered from the originals.

"""
Python bindings for the Tianyan quantum cloud platform client.

This package provides synchronous access to the Tianyan quantum computing
cloud platform, allowing users to:

- Authenticate with API keys
- Discover available quantum backends
- Submit quantum circuits (QCIS format)
- Retrieve execution results with readout error mitigation

All blocking operations release the GIL, making the library safe for use
in multi-threaded Python applications.

Example:
    >>> from cqlib_tianyan import TianyanPlatform, TianyanError
    >>> platform = TianyanPlatform.login("your_api_key")
    >>> backends = platform.list_backends()
    >>> for b in backends:
    ...     print(f"{b.name}: {b.status}")
"""

from typing import List, Optional, final
from cqlib.device import ExecutionResult, Device

@final
class TianyanError(RuntimeError):
    """
    Exception raised for Tianyan platform operation failures.

    This exception is raised when authentication fails, network errors occur,
    or the platform returns an error response. It can be subclassed for
    more specific error types.

    Example:
        >>> try:
        ...     platform = TianyanPlatform.login("invalid_key")
        ... except TianyanError as e:
        ...     print(f"Login failed: {e}")
    """

    pass

@final
class DeviceStatus:
    """
    Operational status of a Tianyan quantum backend.

    Attributes:
        value: The status as a string.

    Possible values:
        - "running": Device is online and accepting jobs.
        - "calibration": Device is being calibrated; submissions may queue.
        - "under_maintenance": Temporarily unavailable for maintenance.
        - "offline": Device is offline.
        - "upgrading": Device is undergoing an upgrade.
        - "unknown": Unrecognized status code from the API.
    """

    value: str

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

@final
class DeviceType:
    """Backend technology classified locally from the machine code.

    Values: "superconducting", "photonic", "ion_trap", "simulator".
    """

    value: str

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

@final
class DeviceToll:
    """
    Pricing model of a Tianyan quantum backend.

    Attributes:
        value: The pricing model as a string.

    Possible values:
        - "free": No charge for submitting jobs.
        - "paid": Job submission consumes credits.
        - "unknown": Unrecognized pricing code from the API.
    """

    value: str

    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...
    def __eq__(self, other: object) -> bool: ...

@final
class CalibrationMode:
    """
    Controls readout error mitigation when fetching task results.

    Args:
        value: One of "auto", "enabled", "disabled".

    Modes:
        - "auto": On superconducting devices, apply mitigation if data is available
          and at most 14 qubits are measured; otherwise return raw counts (default).
          Non-superconducting devices skip configuration download and calibration.
        - "enabled": Require a superconducting device and calibration data;
          other device types fail before submission.
        - "disabled": Never apply mitigation; always return raw counts.

    Raises:
        ValueError: If the string is not one of the recognized values.

    Example:
        >>> mode = CalibrationMode("auto")
        >>> print(mode)  # CalibrationMode('auto')
    """

    value: str

    def __init__(self, value: str) -> None: ...
    def __repr__(self) -> str: ...
    def __str__(self) -> str: ...

@final
class TianyanConfig:
    """
    Runtime configuration for the Tianyan platform client.

    All parameters are optional and default to the official production values.

    Args:
        domain: Platform hostname (default: "qc.zdxlz.com").
        save_credentials: Whether to persist credentials to disk (default: True).
        auto_refresh: Whether to re-login automatically on token expiry (default: True).
        credentials_path: File path for the JSON credentials file
            (default: ~/.cqlib/tianyan/credentials.json).

    Example:
        >>> # Use defaults (official production endpoint)
        >>> cfg = TianyanConfig()
        >>>
        >>> # Custom domain
        >>> cfg = TianyanConfig(domain="my-domain.com")
        >>>
        >>> # Disable credential persistence
        >>> cfg = TianyanConfig(save_credentials=False, auto_refresh=False)
    """

    base_url: str
    """The base URL constructed from scheme + domain."""

    domain: str
    """The configured hostname."""

    save_credentials: bool
    """Whether credentials are saved to disk after login/refresh."""

    auto_refresh: bool
    """Whether an expired token triggers an automatic re-login."""

    credentials_path: str
    """The credentials file path."""

    def __init__(
        self,
        *,
        domain: Optional[str] = None,
        save_credentials: Optional[bool] = None,
        auto_refresh: Optional[bool] = None,
        credentials_path: Optional[str] = None,
    ) -> None: ...
    def __repr__(self) -> str: ...

@final
class TianyanBackend:
    """
    A quantum computing backend available on the Tianyan cloud platform.

    Obtained via `TianyanPlatform.list_backends()` or
    `TianyanPlatform.get_backend()`.

    Example:
        >>> backend = platform.get_backend("tianyan-287")
        >>> if backend.is_available():
        ...     task = backend.run(["H Q1\\nM Q1"], shots=1000)
        ...     results = task.wait(timeout=120.0)
    """

    name: str
    """Machine code used as the backend identifier in submissions."""

    display_name: str
    """User-friendly display name."""

    device_type: DeviceType
    """Backend technology classified locally from the machine code."""

    status: DeviceStatus
    """Current operational status."""

    toll: DeviceToll
    """Pricing model."""

    def is_available(self) -> bool:
        """
        Returns True when the backend is in "running" status.

        Returns:
            bool: Whether the backend is running. Submission also requires a
                superconducting device or simulator.
        """
        ...

    def num_qubits(self) -> int:
        """
        Return the total number of physical qubits in the backend configuration.

        This may download the backend configuration on first use. Disabled
        qubits are included in this count. Only superconducting devices support
        configuration access; other types raise an error.
        """
        ...

    def run(
        self,
        circuits: List[str],
        shots: int,
        *,
        calibration_mode: str | CalibrationMode = "auto",
    ) -> TaskHandle:
        """
        Submit circuits and return a task handle.

        Only superconducting devices and simulators can submit tasks. In the
        default "auto" mode, superconducting devices apply mitigation when data
        is available and at most 14 qubits are measured. Simulators return raw counts.

        Args:
            circuits: List of QCIS circuit strings.
            shots: Number of measurement shots per circuit.
            calibration_mode: "auto", "enabled", or "disabled", as a string or
                CalibrationMode object. Keyword-only; defaults to "auto".
                "enabled" rejects non-superconducting devices before submission.
                Missing calibration data raises an error when retrieving results.

        Returns:
            TaskHandle: A handle that can be used to poll for results.

        Raises:
            TianyanError: If submission fails.
        """
        ...

    def run_raw(self, circuits: List[str], shots: int) -> TaskHandle:
        """
        Deprecated alias for `run(..., calibration_mode="disabled")`.
        Emits DeprecationWarning.

        Args:
            circuits: List of QCIS circuit strings.
            shots: Number of measurement shots per circuit.

        Returns:
            TaskHandle: A handle that can be used to poll for results.
        """
        ...

    def run_with_mode(
        self,
        circuits: List[str],
        shots: int,
        mode: str | CalibrationMode | None = None,
    ) -> TaskHandle:
        """
        Deprecated alias for `run(..., calibration_mode=mode)`.
        Emits DeprecationWarning; None selects "auto".
        "enabled" requires a superconducting device; other types fail before submission.

        Args:
            circuits: List of QCIS circuit strings.
            shots: Number of measurement shots per circuit.
            mode: "auto", "enabled", "disabled", or a CalibrationMode object.
                None defaults to "auto".

        Returns:
            TaskHandle: A handle that can be used to poll for results.

        Raises:
            ValueError: If mode is not a recognized value.
            TianyanError: If submission fails.
        """
        ...

    def device_config(self) -> "Device":
        """
        Download the device calibration configuration (superconducting devices only).

        Returns a `cqlib.device.Device` object populated with topology,
        qubit properties, gate errors, and readout fidelities.

        The result is cached after the first call.

        Returns:
            Device: A cqlib Device object.

        Raises:
            TianyanError: If the configuration cannot be retrieved.
        """
        ...

    def __repr__(self) -> str: ...

@final
class TaskHandle:
    """
    A batch of circuits submitted to the Tianyan quantum cloud platform.

    Obtained from `TianyanBackend.run()` or `TianyanPlatform.submit()`.
    Use `wait()` to block until all results are available, or `status()` for
    a non-blocking snapshot of available results.

    Results are returned as `cqlib.device.ExecutionResult` objects.

    Example:
        >>> task = backend.run(["H Q1\\nM Q1"], shots=1000)
        >>>
        >>> # Non-blocking snapshot
        >>> partial = task.status()
        >>>
        >>> # Block until all results are ready
        >>> results = task.wait(timeout=120.0, poll_interval=5.0)
        >>> for r in results:
        ...     print(r.task_id, r.counts, r.probabilities)
    """

    task_ids: List[str]
    """Platform-assigned query IDs for all submitted circuits."""

    device_name: str
    """Name of the backend device used for the submission."""

    shots: int
    """Number of shots requested per circuit."""

    submitted_at: str
    """Submission timestamp in ISO 8601 format."""

    def status(self) -> List[ExecutionResult]:
        """
        Query the platform once and return whichever results are ready.

        Circuits that have not completed yet are absent from the returned list.
        For polling until all circuits complete, use `wait()` instead.

        Returns:
            List[ExecutionResult]: Results for completed circuits.

        Raises:
            TianyanError: If the query fails.
        """
        ...

    def wait(
        self,
        timeout: float = 120.0,
        poll_interval: float = 5.0,
    ) -> List[ExecutionResult]:
        """
        Block until all submitted circuits have results, then return them.

        The GIL is released while waiting so other Python threads remain active.

        Readout error mitigation is applied according to the calibration mode set
        when the task was submitted (default: "auto").

        Args:
            timeout: Maximum wall-clock time to wait, in seconds (default: 120.0).
            poll_interval: Time between consecutive poll requests, in seconds (default: 5.0).

        Returns:
            List[ExecutionResult]: Results for all submitted circuits.

        Raises:
            TianyanError: If the timeout is exceeded before all results are available.
        """
        ...

    def wait_raw(
        self,
        timeout: float = 120.0,
        poll_interval: float = 5.0,
    ) -> List[ExecutionResult]:
        """
        Like `wait()`, but always returns raw (uncalibrated) counts.

        Args:
            timeout: Maximum wall-clock time to wait, in seconds (default: 120.0).
            poll_interval: Time between consecutive poll requests, in seconds (default: 5.0).

        Returns:
            List[ExecutionResult]: Results for all submitted circuits with raw counts.

        Raises:
            TianyanError: If the timeout is exceeded before all results are available.
        """
        ...

    def __repr__(self) -> str: ...

@final
class TianyanPlatform:
    """
    Synchronous client for the Tianyan quantum cloud platform.

    All methods use blocking I/O. The GIL is released during `wait()` and
    `wait_raw()` polling so other Python threads remain active.

    Example:
        >>> # First-time login — credentials saved to disk by default
        >>> platform = TianyanPlatform.login("your_api_key")
        >>>
        >>> # In-memory only (no disk writes)
        >>> platform = TianyanPlatform.login("your_api_key", save_credentials=False)
        >>>
        >>> # Subsequent runs — reload from disk
        >>> platform = TianyanPlatform.from_credentials()
        >>>
        >>> # Discover backends
        >>> for b in platform.list_backends():
        ...     print(b.name, b.status)
        >>>
        >>> # Submit circuits
        >>> task = platform.submit(["H Q1\\nM Q1"], shots=1000, device_name="tianyan-287")
        >>> results = task.wait(timeout=120.0)
    """

    @staticmethod
    def login(
        api_key: str,
        *,
        domain: Optional[str] = None,
        save_credentials: Optional[bool] = None,
        auto_refresh: Optional[bool] = None,
        credentials_path: Optional[str] = None,
    ) -> "TianyanPlatform":
        """
        Authenticate with an API key and return a platform client.

        Credentials are saved to ~/.cqlib/tianyan/credentials.json by default
        so subsequent calls can use `from_credentials()`.

        Args:
            api_key: Your Tianyan platform API key (openId).
            domain: Platform hostname (default: "qc.zdxlz.com").
            save_credentials: Persist credentials to disk (default: True).
            auto_refresh: Re-login automatically on token expiry (default: True).
            credentials_path: Custom path for the credentials JSON file.

        Returns:
            TianyanPlatform: An authenticated platform client.

        Raises:
            TianyanError: On authentication failure or network error.
        """
        ...

    @staticmethod
    def from_credentials(
        *,
        domain: Optional[str] = None,
        save_credentials: Optional[bool] = None,
        auto_refresh: Optional[bool] = None,
        credentials_path: Optional[str] = None,
    ) -> "TianyanPlatform":
        """
        Load previously saved credentials from disk and return a platform client.

        If the stored access token has expired, re-logs in automatically
        using the saved API key (requires auto_refresh=True, the default).

        Args:
            domain: Platform hostname (default: "qc.zdxlz.com").
            save_credentials: Persist refreshed credentials to disk (default: True).
            auto_refresh: Re-login automatically on token expiry (default: True).
            credentials_path: Custom path for the credentials JSON file.

        Returns:
            TianyanPlatform: An authenticated platform client.

        Raises:
            TianyanError: If the credentials file is missing or the token has
                expired and auto_refresh=False.
        """
        ...

    def list_backends(self) -> List[TianyanBackend]:
        """
        Fetch the full list of available quantum backends from the platform.

        Returns:
            List[TianyanBackend]: List of available backends.

        Raises:
            TianyanError: If the request fails.
        """
        ...

    def get_backend(self, name: str) -> TianyanBackend:
        """
        Return the backend with the given name.

        Args:
            name: Case-sensitive backend identifier (e.g. "tianyan-287").

        Returns:
            TianyanBackend: The requested backend.

        Raises:
            TianyanError: If no backend with that name exists.
        """
        ...

    def submit(
        self,
        circuits: List[str],
        shots: int,
        device_name: str,
        *,
        calibration_mode: str | CalibrationMode = "auto",
    ) -> TaskHandle:
        """
        Submit circuits without fetching a backend handle first.

        Equivalent to `platform.get_backend(device_name).run(circuits, shots,
        calibration_mode=calibration_mode)`,
        but skips the extra device-list network round-trip.

        Args:
            circuits: List of QCIS circuit strings.
            shots: Number of measurement shots per circuit.
            device_name: Target backend identifier.
            calibration_mode: Keyword-only calibration policy; accepts "auto"
                (default), "enabled", "disabled", or a CalibrationMode object.
                Uses the same device restrictions as TianyanBackend.run().

        Returns:
            TaskHandle: A handle for tracking the submission.

        Raises:
            TianyanError: If submission fails.
        """
        ...

    def __repr__(self) -> str: ...

__all__ = [
    "TianyanError",
    "DeviceStatus",
    "DeviceToll",
    "DeviceType",
    "CalibrationMode",
    "TianyanConfig",
    "TianyanBackend",
    "TaskHandle",
    "TianyanPlatform",
]
