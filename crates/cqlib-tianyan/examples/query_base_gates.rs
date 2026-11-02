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
// Added as an example of querying a device's base gates.

use cqlib_tianyan::{DeviceQuery, TianyanPlatform, config::TianyanConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let code = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tianyan-287".to_string());
    let config = TianyanConfig::default().with_save_credentials(false);
    let platform = TianyanPlatform::from_credentials_with_config(config)?;

    // The query API filters by numeric computer ID, not by machine code.
    let all = platform.query_backends(&DeviceQuery::default())?;
    let found = all
        .iter()
        .find(|backend| backend.name == code)
        .ok_or_else(|| format!("device {code} was not returned by the query endpoint"))?;
    let id = found.query_details().unwrap().id.parse::<u64>()?;
    let selected = platform.query_backends(&DeviceQuery::default().with_computer_ids(vec![id]))?;
    let backend = selected
        .iter()
        .find(|backend| backend.name == code)
        .ok_or_else(|| format!("device {code} was not returned by the ID-filtered query"))?;

    println!("{} ({})", backend.name, backend.display_name);
    let gates = backend.base_gates().unwrap_or(&[]);
    println!("baseGate: {} entries", gates.len());
    for gate in gates {
        println!(
            "{} | rule: {}",
            gate.qcis.as_deref().unwrap_or("<missing qcis>"),
            if gate.rule.is_some() {
                "present"
            } else {
                "missing"
            }
        );
    }
    Ok(())
}
