// This code is part of Cqlib.
// (C) Copyright China Telecom Quantum Group 2026
// Licensed under the Apache License, Version 2.0.

use crate::auth::Credentials;
use crate::client::TianyanClient;
use crate::config::{DEVICE_LIST_PATH, DOWNLOAD_CONFIG_PATH, QUERY_RESULT_PATH, SUBMIT_PATH};
use crate::device_config::{
    ReadoutCalibrationData, download_device_config, download_device_config_full,
};
use crate::{
    CalibrationMode, DeviceType, TaskHandle, TianyanConfig, TianyanError, TianyanPlatform,
};
use cqlib_core::device::result::{ExecutionResult, Outcome};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Local HTTP fixture: records requests and serves queued response data.
/// Unexpected requests receive a non-retryable error, including config downloads
/// during simulator execution. Never reads real credentials or contacts the cloud.
struct MockApi {
    client: Arc<TianyanClient>,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MockApi {
    fn new(data: Vec<Value>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let config = TianyanConfig {
            scheme: "http".into(),
            domain: listener.local_addr().unwrap().to_string(),
            save_credentials: false,
            auto_refresh: false,
            ..TianyanConfig::default()
        };
        let credentials = Credentials {
            api_key: "test-key".into(),
            access_token: "test-token".into(),
            token_obtained_at: time::OffsetDateTime::now_utc().unix_timestamp(),
        };
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let requests = requests.clone();
            let stop = stop.clone();
            thread::spawn(move || {
                let mut data: VecDeque<_> = data.into();
                while !stop.load(Ordering::Relaxed) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(e) => panic!("mock accept failed: {e}"),
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut reader = BufReader::new(&mut stream);
                    let mut first_line = String::new();
                    reader.read_line(&mut first_line).unwrap();
                    let mut content_length = 0;
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':') {
                            if name.eq_ignore_ascii_case("content-length") {
                                content_length = value.trim().parse().unwrap();
                            }
                        }
                    }
                    let mut body = vec![0; content_length];
                    reader.read_exact(&mut body).unwrap();
                    requests.lock().unwrap().push((
                        first_line.trim().to_string(),
                        serde_json::from_slice(&body).unwrap_or(Value::Null),
                    ));
                    let (status, response) = match data.pop_front() {
                        Some(value) => ("200 OK", json!({"code": 0, "data": value})),
                        None => (
                            "400 Bad Request",
                            json!({"code": 1, "message": "unexpected request"}),
                        ),
                    };
                    let response = response.to_string();
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
                }
            })
        };
        Self {
            client: Arc::new(TianyanClient::new(config, credentials).unwrap()),
            requests,
            stop,
            worker: Some(worker),
        }
    }

    fn platform(&self) -> TianyanPlatform {
        TianyanPlatform {
            client: self.client.clone(),
        }
    }

    fn assert_paths(&self, expected: &[String]) {
        let actual: Vec<_> = self
            .requests
            .lock()
            .unwrap()
            .iter()
            .map(|(line, _)| line.clone())
            .collect();
        assert_eq!(actual, expected);
    }
}

impl Drop for MockApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn devices() -> Value {
    json!([
        {"code": "tianyan_sw", "status": 0},
        {"code": "tianyan-p2000", "status": 0},
        {"code": "tianyan-ion12", "status": 0},
        {"code": "tianyan504", "status": 0}
    ])
}

fn config_data() -> Value {
    json!({
        "overview": {"qubits": ["Q0"], "couplers": [], "coupler_map": {}},
        "readout": {"readoutArray": {
            "|0> readout fidelity": {"qubit_used": ["Q0"], "param_list": [0.8]},
            "|1> readout fidelity": {"qubit_used": ["Q0"], "param_list": [0.9]}
        }}
    })
}

fn result_data() -> Value {
    json!({"experimentResultModelList": [{
        "query_id": "test-task", "resultStatus": [[0], [0], [0], [0], [1]]
    }]})
}

fn handle(api: &MockApi, name: &str, mode: CalibrationMode) -> TaskHandle {
    TaskHandle {
        task_ids: vec!["test-task".into()],
        device_name: name.into(),
        shots: 4,
        submitted_at: time::OffsetDateTime::now_utc(),
        calibration_mode: mode,
        client: api.client.clone(),
    }
}

fn calibration_data() -> ReadoutCalibrationData {
    crate::device_config::extract_readout_calibration(&config_data()).unwrap()
}

fn assert_raw(results: &[ExecutionResult]) {
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].counts()[&Outcome::from_bitstring("0").unwrap()],
        3
    );
    assert_eq!(
        results[0].counts()[&Outcome::from_bitstring("1").unwrap()],
        1
    );
}

#[test]
fn non_superconducting_config_access_never_sends_download_requests() {
    let api = MockApi::new(vec![devices()]);
    for backend in api.platform().list_backends().unwrap() {
        if backend.device_type == DeviceType::Superconducting {
            continue;
        }
        assert!(matches!(
            backend.num_qubits(),
            Err(TianyanError::InvalidInput(_))
        ));
        assert!(matches!(
            backend.with_device(|_| ()),
            Err(TianyanError::InvalidInput(_))
        ));
        assert!(matches!(
            backend.readout_calibration_data(),
            Err(TianyanError::InvalidInput(_))
        ));
    }
    for name in [
        "tianyan_sw",
        "tianyan-p2000",
        "tianyan-ion12",
        "supremacy_sample",
    ] {
        assert!(matches!(
            download_device_config(&api.client, name),
            Err(TianyanError::InvalidInput(_))
        ));
        assert!(matches!(
            download_device_config_full(&api.client, name),
            Err(TianyanError::InvalidInput(_))
        ));
    }
    api.assert_paths(&[format!("GET {DEVICE_LIST_PATH} HTTP/1.1")]);
}

#[test]
fn superconducting_config_is_downloaded_and_cached() {
    let api = MockApi::new(vec![devices(), config_data()]);
    let backend = api.platform().get_backend("tianyan504").unwrap();
    assert_eq!(backend.num_qubits().unwrap(), 1);
    assert!(backend.readout_calibration_data().unwrap().is_some());
    api.assert_paths(&[
        format!("GET {DEVICE_LIST_PATH} HTTP/1.1"),
        format!("GET {DOWNLOAD_CONFIG_PATH}/tianyan504 HTTP/1.1"),
    ]);
}

#[test]
fn unsupported_submissions_fail_before_http_in_all_modes() {
    let api = MockApi::new(vec![devices()]);
    let platform = api.platform();
    for backend in platform.list_backends().unwrap() {
        if backend.device_type == DeviceType::Superconducting {
            continue;
        }
        for mode in [
            CalibrationMode::Auto,
            CalibrationMode::Enabled,
            CalibrationMode::Disabled,
        ] {
            if backend.device_type == DeviceType::Simulator && mode != CalibrationMode::Enabled {
                continue;
            }
            let error = backend
                .run_with_mode(vec!["M Q0".into()], 4, mode)
                .unwrap_err();
            assert!(matches!(error, TianyanError::InvalidInput(_)));
            assert!(error.to_string().contains(&backend.name));
        }
    }
    for name in ["tianyan-p2000", "tianyan-ion12", "supremacy_sample"] {
        assert!(platform.submit(vec!["M Q0".into()], 4, name).is_err());
    }
    api.assert_paths(&[format!("GET {DEVICE_LIST_PATH} HTTP/1.1")]);
}

#[test]
fn simulator_submission_and_wait_skip_configuration_download() {
    for mode in [CalibrationMode::Auto, CalibrationMode::Disabled] {
        let api = MockApi::new(vec![
            devices(),
            json!({"query_ids": ["test-task"]}),
            result_data(),
        ]);
        let backend = api.platform().get_backend("tianyan_sw").unwrap();
        let task = backend.run_with_mode(vec!["M Q0".into()], 4, mode).unwrap();
        assert_eq!(task.calibration_mode, mode);
        assert_raw(&task.wait(Duration::from_secs(1), Duration::ZERO).unwrap());
        api.assert_paths(&[
            format!("GET {DEVICE_LIST_PATH} HTTP/1.1"),
            format!("POST {SUBMIT_PATH} HTTP/1.1"),
            format!("POST {QUERY_RESULT_PATH} HTTP/1.1"),
        ]);
        assert_eq!(
            api.requests.lock().unwrap()[1].1["computerCode"],
            "tianyan_sw"
        );
    }
}

#[test]
fn superconducting_wait_downloads_and_applies_calibration() {
    for mode in [CalibrationMode::Auto, CalibrationMode::Enabled] {
        let api = MockApi::new(vec![
            json!({"query_ids": ["test-task"]}),
            config_data(),
            result_data(),
        ]);
        let mut task = api
            .platform()
            .submit(vec!["M Q0".into()], 4, "tianyan504")
            .unwrap();
        task.calibration_mode = mode;
        let results = task.wait(Duration::from_secs(1), Duration::ZERO).unwrap();
        assert_eq!(
            results[0].counts()[&Outcome::from_bitstring("0").unwrap()],
            4
        );
        api.assert_paths(&[
            format!("POST {SUBMIT_PATH} HTTP/1.1"),
            format!("GET {DOWNLOAD_CONFIG_PATH}/tianyan504 HTTP/1.1"),
            format!("POST {QUERY_RESULT_PATH} HTTP/1.1"),
        ]);
    }
}

#[test]
fn explicit_calibration_apis_respect_type_and_disabled_mode() {
    for (name, mode) in [
        ("tianyan_sw", CalibrationMode::Auto),
        ("tianyan-p2000", CalibrationMode::Auto),
        ("tianyan-ion12", CalibrationMode::Auto),
        ("tianyan504", CalibrationMode::Disabled),
    ] {
        let api = MockApi::new(vec![result_data(); 3]);
        let task = handle(&api, name, mode);
        assert_raw(&task.status_with_calibration(&calibration_data()).unwrap());
        assert_raw(
            &task
                .wait_with_calibration(Duration::from_secs(1), Duration::ZERO, &calibration_data())
                .unwrap(),
        );
        assert_raw(
            &task
                .wait_calibrated(Duration::from_secs(1), Duration::ZERO)
                .unwrap(),
        );
        api.assert_paths(&vec![format!("POST {QUERY_RESULT_PATH} HTTP/1.1"); 3]);
    }
}

#[test]
fn enabling_calibration_on_existing_non_superconducting_tasks_fails_before_http() {
    let api = MockApi::new(vec![]);
    for name in ["tianyan_sw", "tianyan-p2000", "tianyan-ion12"] {
        let mut task = handle(&api, name, CalibrationMode::Auto);
        task.calibration_mode = CalibrationMode::Enabled;
        assert!(matches!(
            task.wait(Duration::ZERO, Duration::ZERO),
            Err(TianyanError::InvalidInput(_))
        ));
        assert!(matches!(
            task.wait_calibrated(Duration::ZERO, Duration::ZERO),
            Err(TianyanError::InvalidInput(_))
        ));
        assert!(matches!(
            task.status_with_calibration(&calibration_data()),
            Err(TianyanError::InvalidInput(_))
        ));
        assert!(matches!(
            task.wait_with_calibration(Duration::ZERO, Duration::ZERO, &calibration_data()),
            Err(TianyanError::InvalidInput(_))
        ));
    }
    api.assert_paths(&[]);
}

#[test]
fn superconducting_missing_calibration_falls_back_only_in_auto_mode() {
    let config = json!({"overview": {"qubits": ["Q0"], "couplers": [], "coupler_map": {}}});
    let api = MockApi::new(vec![config.clone(), result_data(), config]);
    let mut task = handle(&api, "tianyan504", CalibrationMode::Auto);
    assert_raw(&task.wait(Duration::from_secs(1), Duration::ZERO).unwrap());
    task.calibration_mode = CalibrationMode::Enabled;
    assert!(matches!(
        task.wait(Duration::ZERO, Duration::ZERO),
        Err(TianyanError::InvalidInput(_))
    ));
    api.assert_paths(&[
        format!("GET {DOWNLOAD_CONFIG_PATH}/tianyan504 HTTP/1.1"),
        format!("POST {QUERY_RESULT_PATH} HTTP/1.1"),
        format!("GET {DOWNLOAD_CONFIG_PATH}/tianyan504 HTTP/1.1"),
    ]);
}
