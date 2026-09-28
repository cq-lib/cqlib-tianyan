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
// Modified to cover the upgrading backend status.
// Modified to verify filtering and classification of backend API records.

use super::*;

#[test]
fn device_status_mapping() {
    assert_eq!(DeviceStatus::from_code(0), DeviceStatus::Running);
    assert_eq!(DeviceStatus::from_code(1), DeviceStatus::Calibration);
    assert_eq!(DeviceStatus::from_code(2), DeviceStatus::UnderMaintenance);
    assert_eq!(DeviceStatus::from_code(3), DeviceStatus::OffLine);
    assert_eq!(DeviceStatus::from_code(4), DeviceStatus::Upgrading);
    assert!(matches!(
        DeviceStatus::from_code(99),
        DeviceStatus::Unknown(99)
    ));
}

#[test]
fn device_toll_mapping() {
    assert_eq!(DeviceToll::from_code(1), DeviceToll::Free);
    assert_eq!(DeviceToll::from_code(2), DeviceToll::Paid);
    assert!(matches!(DeviceToll::from_code(0), DeviceToll::Unknown(0)));
}

#[test]
fn backend_records_exclude_non_tianyan_devices() {
    let raw: Vec<RawDevice> = serde_json::from_value(serde_json::json!([
        {"code": "supremacy_sample", "status": 0, "isToll": 1},
        {"code": "tianyan_sw", "status": 0, "isToll": 1},
        {"code": "tianyan-p2000", "status": 0, "isToll": 1},
        {"code": "tianyan-ion12", "status": 2, "isToll": 1},
        {"code": "tianyan504", "status": 4, "isToll": 2},
        {"code": "tianyan-287", "status": 1, "isToll": 2},
        {"code": "other-tianyan", "status": 0, "isToll": 1},
        {"code": "TIANYAN_sw", "status": 0, "isToll": 1},
        {"code": "", "status": 0, "isToll": 1}
    ]))
    .unwrap();
    let client = Arc::new(
        TianyanClient::new(
            crate::TianyanConfig::default().with_save_credentials(false),
            crate::auth::Credentials {
                api_key: "unused".into(),
                access_token: "unused".into(),
                token_obtained_at: 0,
            },
        )
        .unwrap(),
    );
    let backends: Vec<_> = raw
        .into_iter()
        .filter_map(|record| TianyanBackend::from_raw(record, client.clone()))
        .collect();
    let classified: Vec<_> = backends
        .iter()
        .map(|backend| (backend.name.as_str(), backend.device_type))
        .collect();
    assert_eq!(
        classified,
        vec![
            ("tianyan_sw", DeviceType::Simulator),
            ("tianyan-p2000", DeviceType::Photonic),
            ("tianyan-ion12", DeviceType::IonTrap),
            ("tianyan504", DeviceType::Superconducting),
            ("tianyan-287", DeviceType::Superconducting),
        ]
    );
}

#[test]
fn circuit_input_from_str() {
    let input: CircuitInput = "H Q0\nM Q0".into();
    let qcis = input.into_qcis().unwrap();
    assert_eq!(qcis, "H Q0\nM Q0");
}

#[test]
fn circuit_input_from_string() {
    let input: CircuitInput = "H Q0".to_string().into();
    let qcis = input.into_qcis().unwrap();
    assert_eq!(qcis, "H Q0");
}

#[test]
fn device_status_all_known_codes() {
    // Exhaustively check that known codes 0-4 map correctly
    let codes_and_expected = [
        (0, DeviceStatus::Running),
        (1, DeviceStatus::Calibration),
        (2, DeviceStatus::UnderMaintenance),
        (3, DeviceStatus::OffLine),
        (4, DeviceStatus::Upgrading),
    ];
    for (code, expected) in codes_and_expected {
        assert_eq!(DeviceStatus::from_code(code), expected, "code={}", code);
    }
}

#[test]
fn device_status_negative_code() {
    let status = DeviceStatus::from_code(-1);
    assert!(matches!(status, DeviceStatus::Unknown(-1)));
}

#[test]
fn device_toll_negative_code() {
    let toll = DeviceToll::from_code(-1);
    assert!(matches!(toll, DeviceToll::Unknown(-1)));
}

#[test]
fn circuit_input_multiline_qcis() {
    let qcis = "H Q0\nCZ Q0 Q1\nM Q0\nM Q1";
    let input: CircuitInput = qcis.into();
    assert_eq!(input.into_qcis().unwrap(), qcis);
}

#[test]
fn circuit_input_empty_string() {
    let input: CircuitInput = "".into();
    assert_eq!(input.into_qcis().unwrap(), "");
}
