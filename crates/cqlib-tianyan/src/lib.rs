// This code is part of Cqlib.
//
// (C) Copyright China Telecom Quantum Group 2026
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.
//
// Any modifications or derivative works of this code must retain this
// copyright notice, and modified files need to carry a notice indicating
// that they have been altered from the originals.

//! # cqlib-tianyan
//!
//! Rust client for the **Tianyan Quantum Cloud Platform** (`qc.zdxlz.com`).
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use cqlib_tianyan::TianyanPlatform;
//! use std::time::Duration;
//!
//! fn main() -> Result<(), cqlib_tianyan::TianyanError> {
//!     // First-time login – stores credentials to ~/.cqlib/tianyan/credentials.json
//!     let api_key = std::env::var("TIANYAN_API_KEY").expect("set TIANYAN_API_KEY");
//!     let platform = TianyanPlatform::login(&api_key)?;
//!
//!     // Subsequent runs – reload from disk (auto-refresh if expired)
//!     // let platform = TianyanPlatform::from_credentials()?;
//!
//!     // List available backends
//!     let backends = platform.list_backends()?;
//!     for b in &backends {
//!         println!("{}: {:?}", b.name, b.status);
//!     }
//!
//!     // Submit a Bell-state circuit (CNOT = H·CZ·H on target qubit)
//!     // H Q1, H Q8, CZ Q1 Q8, H Q8 ≡ H Q1 followed by CNOT Q1→Q8
//!     let backend = platform.get_backend("tianyan-287")?;
//!     let task = backend.run(
//!         vec!["H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8".into()],
//!         1000,
//!     )?;
//!     let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;
//!
//!     for r in &results {
//!         println!("counts: {:?}", r.counts());
//!     }
//!     Ok(())
//! }
//! ```

pub mod auth;
pub mod calibration;
pub mod client;
pub mod config;
pub mod device;
pub mod device_config;
pub mod error;
pub mod platform;
pub mod task;

// Re-export the most commonly used public types at the crate root.
pub use config::TianyanConfig;
pub use device::{CircuitInput, DeviceStatus, DeviceToll, DeviceType, TianyanBackend};
pub use error::TianyanError;
pub use platform::TianyanPlatform;
pub use task::{CalibrationMode, TaskHandle};

#[cfg(test)]
mod capability_test;
