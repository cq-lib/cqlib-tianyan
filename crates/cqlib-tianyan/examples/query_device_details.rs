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
// Added as an example of a combined device query.

use cqlib_tianyan::{DeviceQuery, TianyanPlatform, config::TianyanConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const COMPUTER_ID: u64 = 1968635345322422274;

    let platform = TianyanPlatform::from_credentials_with_config(
        TianyanConfig::default().with_save_credentials(false),
    )?;
    let query = DeviceQuery::default()
        .with_name("天衍-287")
        .with_name_en("Tianyan-287")
        .with_labels("1")
        .with_toll(2)
        .with_computer_ids(vec![COMPUTER_ID]);
    let backends = platform.query_backends(&query)?;
    if backends.len() != 1 || backends[0].name != "tianyan-287" {
        return Err(format!("expected tianyan-287, got {} devices", backends.len()).into());
    }

    let backend = &backends[0];
    let details = backend.query_details().ok_or("missing query details")?;
    println!("code: {}", backend.name);
    println!("name: {}", backend.display_name);
    println!("id: {}", details.id);
    println!("name_en: {:?}", details.name_en);
    println!("labels: {:?}", details.labels);
    println!("toll: {:?}", backend.toll);
    println!("status: {:?}", backend.status);
    println!("bit_width: {:?}", details.bit_width);
    println!("max_shots: {:?}", details.max_shots);
    println!("max_circuit_layer: {:?}", details.max_circuit_layer);
    println!("base_gates: {}", details.base_gates.len());
    println!("derivative_gates: {}", details.derivative_gates.len());
    Ok(())
}
