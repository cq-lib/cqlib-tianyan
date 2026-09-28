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
# Modified to export the backend device type.

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

# Import all types from the native extension module
from cqlib_tianyan._cqlib_tianyan import (
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

__all__ = [
    "CalibrationMode",
    "DeviceStatus",
    "DeviceToll",
    "DeviceType",
    "TaskHandle",
    "TianyanBackend",
    "TianyanConfig",
    "TianyanError",
    "TianyanPlatform",
]
