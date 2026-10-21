// This code is part of Cqlib.
//
// (C) Copyright China Telecom Quantum Group 2026
//
// This code is licensed under the Apache License, Version 2.0. You may
// obtain a copy of this license in the LICENSE.txt file in the root directory
// of this source tree or at http://www.apache.org/licenses/LICENSE-2.0.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

//! C bindings for the Tianyan quantum cloud platform client.
//!
//! Generated header: `include/cqlib_tianyan.h` (produced by `build.rs` via cbindgen).
//!
//! # Quick start (C)
//!
//! ```c
//! #include "cqlib_tianyan.h"
//! #include <stdio.h>
//!
//! int main(void) {
//!     TianyanPlatform* platform = tianyan_platform_login("your_api_key");
//!     if (!platform) {
//!         char* err = tianyan_last_error();
//!         fprintf(stderr, "Login failed: %s\n", err);
//!         tianyan_string_free(err);
//!         return 1;
//!     }
//!
//!     const char* circuits[] = { "H Q1\nM Q1" };
//!     TianyanTask* task = tianyan_platform_submit(platform, circuits, 1, 1000, "tianyan-287");
//!     TianyanResultList* results = tianyan_task_wait(task, 120.0, 5.0);
//!
//!     char* counts = tianyan_result_counts_json(results, 0);
//!     printf("counts: %s\n", counts);
//!     tianyan_string_free(counts);
//!
//!     tianyan_result_list_free(results);
//!     tianyan_task_free(task);
//!     tianyan_platform_free(platform);
//!     return 0;
//! }
//! ```

pub mod backend;
pub mod config;
pub mod error;
pub mod fusion;
pub mod platform;
pub mod task;
