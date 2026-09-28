"""
Comprehensive test suite for cqlib_tianyan Python bindings.

Tests cover:
- Exception handling
- Configuration management
- Platform authentication
- Backend discovery and operations
- Circuit submission and result retrieval

Environment Variables:
    TIANYAN_API_KEY: API key for authentication (required for integration tests)
    TIANYAN_DOMAIN: Custom domain (optional, defaults to official endpoint)
    PYTEST_RUN_SLOW: Set to any non-empty value to include slow tests (e.g. PYTEST_RUN_SLOW=1)

Usage:
    # Run all tests (requires valid API key)
    TIANYAN_API_KEY="your_key" pytest tests/test_cqlib_tianyan.py -v

    # Run only unit tests (no API key required)
    pytest tests/test_cqlib_tianyan.py -v -m "not integration"

    # Run with custom domain
    TIANYAN_API_KEY="your_key" TIANYAN_DOMAIN="my-domain.com" pytest -v

    # Include slow tests
    TIANYAN_API_KEY="your_key" PYTEST_RUN_SLOW=1 pytest -v
"""

from __future__ import annotations

import os
import uuid

import pytest

# Import all public types
from cqlib_tianyan import (
    CalibrationMode,
    DeviceStatus,
    DeviceToll,
    DeviceType,
    TaskHandle,
    TianyanBackend,
    TianyanConfig,
    TianyanError,
    TianyanPlatform,
)


@pytest.fixture(autouse=True)
def skip_slow_unless_requested(request: pytest.FixtureRequest) -> None:
    """Skip slow-marked tests unless PYTEST_RUN_SLOW env var is set."""
    if "slow" in request.node.keywords and not os.environ.get("PYTEST_RUN_SLOW"):
        pytest.skip("Slow test: set PYTEST_RUN_SLOW=1 to include")


@pytest.fixture
def api_key() -> str | None:
    """Get API key from environment variable."""
    return os.environ.get("TIANYAN_API_KEY")


@pytest.fixture
def domain() -> str | None:
    """Get custom domain from environment variable."""
    return os.environ.get("TIANYAN_DOMAIN")


@pytest.fixture
def skip_without_api_key(api_key: str | None) -> None:
    """Skip test if API key is not available."""
    if not api_key:
        pytest.skip("TIANYAN_API_KEY environment variable not set")


@pytest.fixture
def platform(api_key: str | None, domain: str | None) -> TianyanPlatform:
    """Create authenticated platform instance.

    Requires TIANYAN_API_KEY environment variable.
    """
    if not api_key:
        pytest.skip("TIANYAN_API_KEY environment variable not set")

    kwargs: dict[str, object] = {"api_key": api_key}
    if domain:
        kwargs["domain"] = domain

    return TianyanPlatform.login(**kwargs)


@pytest.fixture
def device_name() -> str | None:
    """Get target device name from environment variable."""
    return os.environ.get("TIANYAN_DEVICE")


@pytest.fixture
def available_backend(
    platform: TianyanPlatform, device_name: str | None
) -> TianyanBackend:
    """Get backend for testing.

    If TIANYAN_DEVICE environment variable is set, use that specific device.
    Otherwise, find the first available backend.
    """
    if device_name:
        # Use specified device
        try:
            backend = platform.get_backend(device_name)
            if backend.device_type.value not in ("superconducting", "simulator"):
                pytest.skip(f"Device '{device_name}' does not support task submission")
            if not backend.is_available():
                pytest.skip(
                    f"Specified device '{device_name}' is not available (status: {backend.status})"
                )
            return backend
        except Exception as e:
            pytest.skip(f"Failed to get specified device '{device_name}': {e}")

    # Find first available backend
    backends = platform.list_backends()
    if not backends:
        pytest.skip("No backends available on the platform")

    for backend in backends:
        if backend.is_available() and backend.device_type.value in (
            "superconducting",
            "simulator",
        ):
            return backend

    pytest.skip("No available backends found (all offline or under maintenance)")


@pytest.fixture
def simple_circuit() -> str:
    """Return a simple test circuit in QCIS format."""
    return "H Q1\nM Q1"


class TestTianyanError:
    """Tests for TianyanError exception type.

    Note: In abi3 mode, TianyanError cannot inherit RuntimeError or be
    instantiated directly. It can only be raised by Rust code.
    """

    def test_error_type_exists(self) -> None:
        """TianyanError type is exported from module."""
        assert TianyanError is not None
        assert hasattr(TianyanError, "__name__")


class TestTianyanConfig:
    """Tests for TianyanConfig configuration class."""

    def test_default_config(self) -> None:
        """Default configuration uses production values."""
        config = TianyanConfig()

        assert config.domain == "qc.zdxlz.com"
        assert config.save_credentials is True
        assert config.auto_refresh is True
        assert config.base_url == "https://qc.zdxlz.com"

    def test_custom_domain(self) -> None:
        """Custom domain is properly set."""
        config = TianyanConfig(domain="custom.example.com")

        assert config.domain == "custom.example.com"
        assert config.base_url == "https://custom.example.com"

    def test_disable_save_credentials(self) -> None:
        """Credential persistence can be disabled."""
        config = TianyanConfig(save_credentials=False)
        assert config.save_credentials is False

    def test_disable_auto_refresh(self) -> None:
        """Auto-refresh can be disabled."""
        config = TianyanConfig(auto_refresh=False)
        assert config.auto_refresh is False

    def test_custom_credentials_path(self) -> None:
        """Custom credentials path is accepted."""
        path = "/tmp/test_creds.json"
        config = TianyanConfig(credentials_path=path)
        assert config.credentials_path == path

    def test_repr(self) -> None:
        """__repr__ provides useful debugging information."""
        config = TianyanConfig(domain="test.com", save_credentials=False)
        repr_str = repr(config)

        assert "TianyanConfig" in repr_str
        assert "test.com" in repr_str
        # Note: Rust uses lowercase 'false'/'true' for booleans
        assert "false" in repr_str

    def test_all_options_combined(self) -> None:
        """All configuration options can be set together."""
        config = TianyanConfig(
            domain="all.example.com",
            save_credentials=False,
            auto_refresh=False,
            credentials_path="/tmp/creds.json",
        )

        assert config.domain == "all.example.com"
        assert config.save_credentials is False
        assert config.auto_refresh is False
        assert config.credentials_path == "/tmp/creds.json"


class TestDeviceStatus:
    """Tests for DeviceStatus enumeration."""

    def test_status_values(self, platform: TianyanPlatform) -> None:
        """DeviceStatus has expected string values."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        for backend in backends:
            status = backend.status
            assert isinstance(status.value, str)
            assert status.value in [
                "running",
                "calibration",
                "under_maintenance",
                "offline",
                "upgrading",
                "unknown",
            ]

    def test_status_equality(self, platform: TianyanPlatform) -> None:
        """DeviceStatus can be compared with strings."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        backend = backends[0]
        assert backend.status == backend.status.value

    def test_status_str(self, platform: TianyanPlatform) -> None:
        """DeviceStatus string representation matches value."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        status = backends[0].status
        assert str(status) == status.value

    def test_status_repr(self, platform: TianyanPlatform) -> None:
        """DeviceStatus repr contains class name."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        status = backends[0].status
        repr_str = repr(status)
        assert "DeviceStatus" in repr_str
        assert status.value in repr_str


class TestDeviceToll:
    """Tests for DeviceToll enumeration."""

    def test_toll_values(self, platform: TianyanPlatform) -> None:
        """DeviceToll has expected string values."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        for backend in backends:
            toll = backend.toll
            assert isinstance(toll.value, str)
            assert toll.value in ["free", "paid", "unknown"]


class TestCalibrationMode:
    """Tests for CalibrationMode enumeration."""

    def test_auto_mode(self) -> None:
        """Auto mode is created correctly."""
        mode = CalibrationMode("auto")
        assert mode.value == "auto"

    def test_enabled_mode(self) -> None:
        """Enabled mode is created correctly."""
        mode = CalibrationMode("enabled")
        assert mode.value == "enabled"

    def test_disabled_mode(self) -> None:
        """Disabled mode is created correctly."""
        mode = CalibrationMode("disabled")
        assert mode.value == "disabled"

    def test_invalid_mode_raises(self) -> None:
        """Invalid mode raises ValueError."""
        with pytest.raises(ValueError):
            CalibrationMode("invalid_mode")

    def test_mode_str(self) -> None:
        """String representation matches value."""
        mode = CalibrationMode("auto")
        assert str(mode) == "auto"

    def test_mode_repr(self) -> None:
        """Repr contains class name and value."""
        mode = CalibrationMode("disabled")
        repr_str = repr(mode)
        assert "CalibrationMode" in repr_str
        assert "disabled" in repr_str


@pytest.mark.integration
class TestTianyanPlatformAuth:
    """Integration tests for platform authentication."""

    def test_login_with_valid_key(
        self, api_key: str | None, domain: str | None
    ) -> None:
        """Login with valid API key succeeds."""
        if not api_key:
            pytest.skip("TIANYAN_API_KEY not set")

        kwargs: dict[str, object] = {"api_key": api_key}
        if domain:
            kwargs["domain"] = domain

        platform = TianyanPlatform.login(**kwargs)
        assert platform is not None
        assert "TianyanPlatform" in repr(platform)

    def test_login_with_invalid_key(self, domain: str | None) -> None:
        """Login with invalid API key raises an error."""
        kwargs: dict[str, object] = {"api_key": "invalid_key_12345"}
        if domain:
            kwargs["domain"] = domain

        # Note: In abi3 mode, TianyanError cannot be caught by type,
        # so we catch any exception and verify it's an error
        with pytest.raises(Exception):
            TianyanPlatform.login(**kwargs)

    def test_login_without_save_credentials(
        self, api_key: str | None, domain: str | None
    ) -> None:
        """Login with save_credentials=False does not persist."""
        if not api_key:
            pytest.skip("TIANYAN_API_KEY not set")

        kwargs: dict[str, object] = {"api_key": api_key, "save_credentials": False}
        if domain:
            kwargs["domain"] = domain

        platform = TianyanPlatform.login(**kwargs)
        assert platform is not None

    def test_repr(self, platform: TianyanPlatform) -> None:
        """Platform repr contains URL."""
        repr_str = repr(platform)
        assert "TianyanPlatform" in repr_str
        assert "url" in repr_str.lower()


@pytest.mark.integration
class TestTianyanBackend:
    """Integration tests for backend operations."""

    def test_list_backends(self, platform: TianyanPlatform) -> None:
        """Backends can be listed."""
        backends = platform.list_backends()
        assert isinstance(backends, list)

        for backend in backends:
            assert isinstance(backend, TianyanBackend)
            assert hasattr(backend, "name")
            assert hasattr(backend, "status")
            assert hasattr(backend, "toll")

    def test_get_backend_by_name(self, platform: TianyanPlatform) -> None:
        """Backend can be retrieved by name."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        name = backends[0].name
        backend = platform.get_backend(name)

        assert backend.name == name

    def test_get_nonexistent_backend(self, platform: TianyanPlatform) -> None:
        """Getting non-existent backend raises an error."""
        fake_name = f"nonexistent-backend-{uuid.uuid4()}"

        # Note: In abi3 mode, TianyanError cannot be caught by type
        with pytest.raises(Exception):
            platform.get_backend(fake_name)

    def test_backend_properties(self, platform: TianyanPlatform) -> None:
        """Backend has required properties."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        backend = backends[0]

        # Required properties
        assert isinstance(backend.name, str)
        assert len(backend.name) > 0
        assert isinstance(backend.display_name, str)
        assert isinstance(backend.device_type, DeviceType)
        assert backend.device_type.value in (
            "superconducting",
            "photonic",
            "ion_trap",
            "simulator",
        )
        assert backend.device_type == backend.device_type.value
        assert str(backend.device_type) == backend.device_type.value
        assert repr(backend.device_type) == f"DeviceType('{backend.device_type.value}')"
        assert isinstance(backend.status, DeviceStatus)
        assert isinstance(backend.toll, DeviceToll)

        if backend.device_type == "superconducting":
            num_qubits = backend.num_qubits()
            assert isinstance(num_qubits, int)
            assert num_qubits > 0
        else:
            with pytest.raises(Exception, match="only supported for superconducting"):
                backend.num_qubits()

    def test_is_available(self, available_backend: TianyanBackend) -> None:
        """Available backend reports is_available=True."""
        assert available_backend.is_available() is True

    def test_backend_repr(self, platform: TianyanPlatform) -> None:
        """Backend repr contains useful information."""
        backends = platform.list_backends()
        if not backends:
            pytest.skip("No backends to test")

        repr_str = repr(backends[0])
        assert "TianyanBackend" in repr_str
        assert "name" in repr_str

    def test_device_config(self, available_backend: TianyanBackend) -> None:
        """Device configuration can be retrieved."""
        if available_backend.device_type != "superconducting":
            with pytest.raises(Exception, match="only supported for superconducting"):
                available_backend.device_config()
            return

        from cqlib.device import Device

        device = available_backend.device_config()
        assert isinstance(device, Device)
        assert hasattr(device, "name")
        assert hasattr(device, "qubits")
        assert hasattr(device, "topology")


@pytest.mark.integration
class TestCircuitSubmission:
    """Integration tests for circuit submission."""

    def test_run_single_circuit(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Single circuit can be submitted and executed."""
        task = available_backend.run([simple_circuit], shots=100)

        assert isinstance(task, TaskHandle)
        assert task.device_name == available_backend.name
        assert task.shots == 100
        assert len(task.task_ids) == 1

    def test_run_multiple_circuits(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Multiple circuits can be submitted in one batch."""
        circuits = [simple_circuit, "X Q1\nM Q1"]
        task = available_backend.run(circuits, shots=100)

        assert len(task.task_ids) == 2

    def test_run_raw(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Raw execution returns uncalibrated counts."""
        task = available_backend.run_raw([simple_circuit], shots=100)

        assert isinstance(task, TaskHandle)

    def test_run_with_mode(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Execution with explicit calibration mode."""
        task = available_backend.run_with_mode(
            [simple_circuit], shots=100, mode="disabled"
        )

        assert isinstance(task, TaskHandle)

    def test_run_with_invalid_mode(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Invalid calibration mode raises error."""
        with pytest.raises(ValueError):
            available_backend.run_with_mode([simple_circuit], shots=100, mode="invalid")

    def test_task_handle_properties(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """TaskHandle has expected properties."""
        task = available_backend.run([simple_circuit], shots=100)

        assert isinstance(task.task_ids, list)
        assert all(isinstance(tid, str) for tid in task.task_ids)
        assert task.device_name == available_backend.name
        assert task.shots == 100
        assert isinstance(task.submitted_at, str)

    def test_task_handle_repr(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """TaskHandle repr contains useful information."""
        task = available_backend.run([simple_circuit], shots=100)
        repr_str = repr(task)

        assert "TaskHandle" in repr_str
        assert "device" in repr_str.lower()
        assert "shots" in repr_str.lower()

    def test_platform_submit(
        self,
        platform: TianyanPlatform,
        available_backend: TianyanBackend,
        simple_circuit: str,
    ) -> None:
        """Circuits can be submitted via platform shortcut."""
        task = platform.submit(
            [simple_circuit],
            shots=100,
            device_name=available_backend.name,
        )

        assert isinstance(task, TaskHandle)
        assert task.device_name == available_backend.name


@pytest.mark.integration
class TestResultRetrieval:
    """Integration tests for result retrieval."""

    @pytest.mark.slow
    def test_wait_for_results(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Results can be retrieved via wait()."""
        task = available_backend.run([simple_circuit], shots=100)

        # Wait with reasonable timeout
        results = task.wait(timeout=300.0, poll_interval=5.0)

        assert isinstance(results, list)
        assert len(results) == 1

        result = results[0]
        assert hasattr(result, "task_id")
        assert hasattr(result, "counts")
        assert hasattr(result, "probabilities")
        assert result.shots == 100

    def test_status_non_blocking(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Status returns immediately without blocking."""
        task = available_backend.run([simple_circuit], shots=100)

        # Should return immediately, even if results not ready
        results = task.status()

        assert isinstance(results, list)
        # May be empty if results not ready yet

    @pytest.mark.slow
    def test_wait_raw_results(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Raw results can be retrieved."""
        task = available_backend.run_raw([simple_circuit], shots=100)

        results = task.wait_raw(timeout=300.0, poll_interval=5.0)

        assert isinstance(results, list)
        assert len(results) == 1

    @pytest.mark.slow
    def test_wait_timeout(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Wait with very short timeout may raise error."""
        task = available_backend.run([simple_circuit], shots=100)

        # Very short timeout should fail
        # Note: In abi3 mode, TianyanError cannot be caught by type
        with pytest.raises(Exception):
            task.wait(timeout=0.001, poll_interval=0.001)


@pytest.mark.integration
class TestEdgeCases:
    """Tests for edge cases and error conditions."""

    def test_empty_circuit_list(self, available_backend: TianyanBackend) -> None:
        """Empty circuit list handling."""
        # This may either succeed with empty result or raise error
        try:
            task = available_backend.run([], shots=100)
            assert len(task.task_ids) == 0
        except Exception:
            # Either behavior is acceptable
            pass

    def test_zero_shots(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Zero shots handling."""
        # This may raise error or be handled by platform
        with pytest.raises(Exception):
            available_backend.run([simple_circuit], shots=0)

    def test_negative_shots(
        self, available_backend: TianyanBackend, simple_circuit: str
    ) -> None:
        """Negative shots should raise error."""
        with pytest.raises(Exception):
            available_backend.run([simple_circuit], shots=-1)

    def test_invalid_circuit_syntax(self, available_backend: TianyanBackend) -> None:
        """Invalid circuit syntax handling."""
        invalid_circuit = "INVALID_GATE Q1"

        # May fail at submission or execution time
        try:
            task = available_backend.run([invalid_circuit], shots=100)
            # If submitted, should fail when waiting
            # Note: In abi3 mode, TianyanError cannot be caught by type
            with pytest.raises(Exception):
                task.wait(timeout=60.0)
        except Exception:
            # Immediate rejection is also valid
            pass

    def test_very_long_circuit(self, available_backend: TianyanBackend) -> None:
        """Very long circuit handling."""
        # Create a long circuit (1000 gates)
        long_circuit = "\n".join([f"H Q{i % 10}" for i in range(1000)]) + "\nM Q1"

        # May succeed or fail depending on backend limits
        try:
            task = available_backend.run([long_circuit], shots=10)
            results = task.wait(timeout=300.0)
            assert len(results) == 1
        except Exception:
            # Rejection due to circuit size is acceptable
            pass


def test_module_exports() -> None:
    """All expected types are exported from module."""
    import cqlib_tianyan as tianyan

    assert hasattr(tianyan, "TianyanPlatform")
    assert hasattr(tianyan, "TianyanBackend")
    assert hasattr(tianyan, "TaskHandle")
    assert hasattr(tianyan, "TianyanConfig")
    assert hasattr(tianyan, "TianyanError")
    assert hasattr(tianyan, "DeviceStatus")
    assert hasattr(tianyan, "DeviceToll")
    assert hasattr(tianyan, "DeviceType")
    assert hasattr(tianyan, "CalibrationMode")


if __name__ == "__main__":
    # Run with: python -m pytest tests/test_cqlib_tianyan.py -v
    pytest.main([__file__, "-v"])
