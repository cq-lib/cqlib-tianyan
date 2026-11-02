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
// Modified to verify filtered device queries and base-gate data.

use super::*;

#[test]
fn device_status_mapping() {
    assert_eq!(DeviceStatus::from_code(0), DeviceStatus::Running);
    assert_eq!(DeviceStatus::from_code(1), DeviceStatus::Calibration);
    assert_eq!(DeviceStatus::from_code(2), DeviceStatus::UnderMaintenance);
    assert_eq!(DeviceStatus::from_code(3), DeviceStatus::OffLine);
    assert_eq!(DeviceStatus::from_code(4), DeviceStatus::Upgrading);
    assert_eq!(DeviceStatus::from_code(5), DeviceStatus::Unknown(5));
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

#[test]
fn query_backends_sends_filters_and_preserves_gate_data() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(false).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut first_line = String::new();
        reader.read_line(&mut first_line).unwrap();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }

        let response = serde_json::json!({
            "code": 200,
            "msg": "操作成功",
            "data": [
                {
                    "id": "1001", "code": "tianyan176", "name": "祖冲之号",
                    "nameEn": "TianYan-176", "status": 0, "isToll": 1,
                    "bitWidth": 176, "qubitNum": 176, "maxShots": 100000,
                    "maxCircuitLayer": 50, "labels": "1",
                    "baseGate": [{"id": "10001", "qcis": "H", "rule": "H Q", "bitWidth": 1, "type": 0}],
                    "derivativeGate": []
                },
                {
                    "id": "1002", "code": "tianyan-287", "name": "示例设备",
                    "status": 5, "isToll": 2, "baseGate": []
                }
            ]
        })
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
        first_line
    });

    let config = crate::TianyanConfig {
        scheme: "http".into(),
        domain: address.to_string(),
        save_credentials: false,
        auto_refresh: false,
        ..crate::TianyanConfig::default()
    };
    let client = Arc::new(
        TianyanClient::new(
            config,
            crate::auth::Credentials {
                api_key: "unused".into(),
                access_token: "test-token".into(),
                token_obtained_at: time::OffsetDateTime::now_utc().unix_timestamp(),
            },
        )
        .unwrap(),
    );
    let query = DeviceQuery::default()
        .with_name("祖冲之")
        .with_name_en("TianYan")
        .with_labels("1")
        .with_toll(1)
        .with_computer_ids(vec![1001, 1002]);
    let backends = query_backends(client.clone(), &query).unwrap();
    let first_line = worker.join().unwrap();
    let target = first_line.split_whitespace().nth(1).unwrap();
    let url = reqwest::Url::parse(&format!("http://localhost{target}")).unwrap();
    assert_eq!(url.path(), DEVICE_QUERY_PATH);
    let pairs: Vec<_> = url
        .query_pairs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("name".into(), "祖冲之".into()),
            ("nameEn".into(), "TianYan".into()),
            ("labels".into(), "1".into()),
            ("isToll".into(), "1".into()),
            ("computerIds".into(), "1001".into()),
            ("computerIds".into(), "1002".into()),
        ]
    );

    assert_eq!(backends.len(), 2);
    assert_eq!(backends[0].name, "tianyan176");
    assert_eq!(backends[0].query_details().unwrap().id, "1001");
    assert_eq!(backends[0].query_details().unwrap().max_shots, Some(100000));
    assert_eq!(
        backends[0].query_details().unwrap().field("qubitNum"),
        Some(&serde_json::json!(176))
    );
    assert_eq!(
        backends[0].base_gates().unwrap()[0].qcis.as_deref(),
        Some("H")
    );
    assert_eq!(backends[1].name, "tianyan-287");
    assert_eq!(backends[1].device_type, DeviceType::Superconducting);
    assert_eq!(backends[1].status, DeviceStatus::Unknown(5));

    let unknown_raw = serde_json::json!({"id": "1003", "code": "QPU-01", "status": 0});
    let unknown_record = serde_json::from_value(unknown_raw.clone()).unwrap();
    assert!(matches!(
        TianyanBackend::from_query_record(unknown_record, unknown_raw, client),
        Err(TianyanError::InvalidInput(message)) if message.contains("QPU-01")
    ));
}

#[test]
fn invalid_toll_filter_is_rejected_before_request() {
    assert!(
        DeviceQuery::default()
            .with_toll(0)
            .query_pairs()
            .unwrap()
            .is_empty()
    );
    let query = DeviceQuery::default().with_toll(3);
    assert!(matches!(
        query.query_pairs(),
        Err(TianyanError::InvalidInput(_))
    ));
}
