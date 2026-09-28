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

//! End-to-end example: login → list devices → inspect calibration → submit → results.
//!
//! # Setup
//!
//! Set your API key before running:
//!
//! ```sh
//! export TIANYAN_API_KEY="your_api_key_here"
//! cargo run -p cqlib-tianyan --example e2e
//! ```

use cqlib_tianyan::TianyanPlatform;
use cqlib_tianyan::config::TianyanConfig;
use std::time::Duration;

fn main() -> Result<(), cqlib_tianyan::TianyanError> {
    // API key from environment variable — never hard-code secrets.
    let api_key = std::env::var("TIANYAN_API_KEY")
        .expect("Set TIANYAN_API_KEY environment variable before running");

    println!("  cqlib-tianyan  E2E  Demo");

    println!("[1/7] Authenticating...");
    let cfg = TianyanConfig::default()
        .with_save_credentials(true)
        .with_auto_refresh(true);
    let platform = TianyanPlatform::login_with_config(&api_key, cfg)?;
    println!("       ✓ Login OK — credentials saved to ~/.cqlib/tianyan/\n");

    println!("[2/7] Available backends:");
    let backends = platform.list_backends()?;
    for b in &backends {
        println!(
            "       - {:25}  status={:?} toll={:?}",
            b.name, b.status, b.toll,
        );
    }
    println!();

    // tianyan-287: Q1 ↔ Q8 are coupled on this machine.
    let device_name = "tianyan-287";
    println!("[3/7] Selecting backend '{}'...", device_name);
    let backend = platform.get_backend(device_name)?;
    println!(
        "       ✓ id={}  status={:?} \n",
        backend.name, backend.status,
    );

    println!("[4/7] Downloading calibration config...");
    let (num_qubits, num_couplings) = backend.with_device(|device| {
        let topo = device.topology();
        (topo.num_qubits(), topo.num_couplings())
    })?;
    println!(
        "       Topology: {} qubits, {} couplings",
        num_qubits, num_couplings
    );
    println!();

    println!("[5/7] Readout calibration fidelities:");
    match backend.readout_calibration_data()? {
        Some(cal) => {
            println!(
                "       {:>6}  {:>8}  {:>8}  {:>12}",
                "Qubit", "f00", "f11", "Error%"
            );
            for (i, name) in cal.qubit_names.iter().enumerate() {
                let f00 = cal.f00[i];
                let f11 = cal.f11[i];
                let err = (1.0 - (f00 + f11) / 2.0) * 100.0;
                println!(
                    "       {:>6}  {:>8.4}  {:>8.4}  {:>11.2}%",
                    name, f00, f11, err
                );
            }
        }
        None => println!("       (no calibration data for this backend)"),
    }
    println!();

    // Bell state |Φ+⟩ = (|00⟩ + |11⟩) / √2
    let circuit = "H Q1\nH Q8\nCZ Q1 Q8\nH Q8\nM Q1\nM Q8";
    let shots = 1000;
    println!(
        "[6/7] Submitting Bell-state circuit ({} shots):\n       {}",
        shots,
        circuit.replace('\n', "  ")
    );
    println!();

    // CalibrationMode::Auto (default) — wait() will apply readout calibration
    let task = backend.run(vec![circuit.into()], shots)?;
    println!(
        "       ✓ Task IDs: {:?}  (calibration_mode={:?})\n",
        task.task_ids(),
        task.calibration_mode
    );

    println!("[7/7] Waiting for results (timeout=120s, interval=5s)...\n");

    let results = task.wait(Duration::from_secs(120), Duration::from_secs(5))?;

    let n_qubits: usize = 2; // Bell circuit measures Q1 and Q8

    for r in &results {
        println!("  Task ID : {}", r.task_id());

        let mut outcomes: Vec<_> = r.counts().keys().collect();
        outcomes.sort_by_key(|o| usize::from_str_radix(&o.to_bitstring(n_qubits), 2).unwrap_or(0));

        println!("  {:>4}  {:>10}  {:>10}", "Basis", "Count", "Prob");
        for o in &outcomes {
            let basis = o.to_bitstring(n_qubits);
            let cc = r.counts().get(*o).copied().unwrap_or(0);
            let cp = r
                .probabilities()
                .as_ref()
                .and_then(|m| m.get(*o))
                .copied()
                .unwrap_or(0.0);
            println!("  {:>4}  {:>10}  {:>10.4}", basis, cc, cp);
        }
        println!();
    }

    println!("✅ E2E demo complete!");
    Ok(())
}
