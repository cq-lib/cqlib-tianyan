use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn server(replies: Vec<(u16, String, String)>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let worker = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, extra, body) in replies {
            let started = Instant::now();
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                if started.elapsed() > Duration::from_secs(4) {
                    return requests;
                }
                thread::sleep(Duration::from_millis(5));
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let count = stream.read(&mut buf).unwrap();
                if count == 0 {
                    break;
                }
                data.extend_from_slice(&buf[..count]);
                if let Some(end) = data.windows(4).position(|p| p == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8_lossy(&data).into_owned());
            let length = if extra.contains("Content-Length:") {
                String::new()
            } else {
                format!("Content-Length: {}\r\n", body.len())
            };
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\n{length}Connection: close\r\n{extra}\r\n{body}"
            )
            .unwrap();
        }
        requests
    });
    (host, worker)
}
fn ok(data: Value) -> (u16, String, String) {
    (
        200,
        String::new(),
        json!({"code":0,"data":data}).to_string(),
    )
}
fn client(host: &str) -> FusionClient {
    FusionClient::connect(host, "test-token", None, 2.0).unwrap()
}
fn options(path: &Path) -> SubmitOptions {
    SubmitOptions {
        name: "demo".into(),
        package: path.into(),
        dispatch_mode: "intelligent".into(),
        dispatch_preference: "balanced".into(),
        quantum_resource: None,
        hpc_resource: None,
        auto_confirm: false,
        plan_id: None,
    }
}

#[test]
fn intelligent_submission_never_enqueues_until_confirmed() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"PK-test").unwrap();
    let (host, server) = server(vec![
        ok(json!({"package_id":"p"})),
        ok(json!({"task_id":"t"})),
        ok(json!({"plans":[{"plan_id":"plan"}]})),
    ]);
    let result = client(&host).submit(&options(file.path())).unwrap();
    assert!(result.awaiting_confirmation);
    assert_eq!(result.job["status"], "awaiting_confirm");
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].contains("multipart/form-data"));
    assert!(requests[0].contains("PK-test"));
    assert!(requests[2].starts_with("POST /qccp-quantum/sdk/fusion/task/recommend"));
    assert!(
        !requests
            .iter()
            .any(|r| r.starts_with("POST /qccp-quantum/sdk/fusion/task/confirm"))
    );
}

#[test]
fn auto_confirm_and_manual_use_different_routes() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let (host, server) = server(vec![
        ok(json!({})),
        ok(json!({"id":"t"})),
        ok(json!([{ "id":"plan" }])),
        ok(json!({"id":"t","status":"queued"})),
    ]);
    let mut opt = options(file.path());
    opt.auto_confirm = true;
    assert!(!client(&host).submit(&opt).unwrap().awaiting_confirmation);
    assert!(server.join().unwrap()[3].contains("\"plan_id\":\"plan\""));
    opt.dispatch_mode = "manual".into();
    // Validate resource bindings before uploading anything.
    assert!(client("http://127.0.0.1:1").submit(&opt).is_err());
    opt.quantum_resource = Some("q".into());
    opt.hpc_resource = Some("h".into());
    let (host, server) = self::server(vec![ok(json!({})), ok(json!({"id":"t"}))]);
    assert!(!client(&host).submit(&opt).unwrap().awaiting_confirmation);
    assert!(server.join().unwrap()[1].starts_with("POST /qccp-quantum/sdk/fusion/task/run"));
}

#[test]
fn mutations_do_not_replay_after_server_error_and_keep_recovery_data() {
    let (host, server) = server(vec![
        (
            503,
            String::new(),
            json!({"code":"UNCONFIRMED","msg":"unknown","data":{"rootId":"88"}}).to_string(),
        ),
        ok(json!({})),
    ]);
    let error = client(&host).confirm("88", "plan").unwrap_err();
    assert_eq!(error.http_status, Some(503));
    assert_eq!(error.data.unwrap()["rootId"], "88");
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn reads_retry_and_delete_checks_business_error() {
    let (host, server) = server(vec![
        (503, String::new(), String::new()),
        ok(json!({"id":"t"})),
        (
            200,
            String::new(),
            json!({"code":409,"msg":"busy","data":{"id":"t"}}).to_string(),
        ),
    ]);
    let c = client(&host);
    assert_eq!(c.get("t").unwrap()["id"], "t");
    assert_eq!(c.delete("t").unwrap_err().code, Some(json!(409)));
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[2].starts_with("DELETE "));
}

#[test]
fn shared_session_refreshes_once_without_persisting_when_disabled() {
    let directory = tempfile::tempdir().unwrap();
    let (host, server) = server(vec![
        (401, String::new(), String::new()),
        ok(json!({"access_token":"renewed"})),
        ok(json!({"id":"t"})),
        ok(json!({"id":"other"})),
    ]);
    let mut config =
        TianyanConfig::custom(host.strip_prefix("http://").unwrap()).with_save_credentials(false);
    config.scheme = "http".into();
    config.credentials_path = directory.path().join("credentials.json");
    let credentials = Credentials {
        api_key: "test-key".into(),
        access_token: "old".into(),
        token_obtained_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    };
    let shared = Arc::new(TianyanClient::new(config, credentials).unwrap());
    let c = FusionClient::from_client(shared.clone());
    c.confirm("t", "plan").unwrap();
    // A second handle observes the token refreshed by the first.
    FusionClient::from_client(shared).get("other").unwrap();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests[1].contains("openId=test-key"));
    assert!(
        requests[2]
            .to_lowercase()
            .contains("authorization: bearer renewed")
    );
    assert!(
        requests[3]
            .to_lowercase()
            .contains("authorization: bearer renewed")
    );
    assert!(!directory.path().join("credentials.json").exists());
}

#[test]
fn refresh_disabled_does_not_login() {
    let (host, server) = server(vec![(401, String::new(), String::new())]);
    assert!(client(&host).get("t").is_err());
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn downloads_sanitize_names_and_preserve_existing_file_on_truncation() {
    let directory = tempfile::tempdir().unwrap();
    let (host, server) = server(vec![(
        200,
        "Content-Disposition: attachment; filename=\"../../escaped.zip\"\r\n".into(),
        "result".into(),
    )]);
    let file = client(&host)
        .download_results("t", directory.path())
        .unwrap();
    assert_eq!(file, directory.path().join("escaped.zip"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "result");
    server.join().unwrap();
    let (host, server) = self::server(vec![(
        200,
        "Content-Length: 100\r\nContent-Disposition: attachment; filename=escaped.zip\r\n".into(),
        "short".into(),
    )]);
    assert!(
        client(&host)
            .download_results("t", directory.path())
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(file).unwrap(), "result");
    server.join().unwrap();
    assert_eq!(
        download_name("attachment; filename*=UTF-8''%E7%BB%93%E6%9E%9C.json"),
        Some("结果.json".into())
    );
}

#[test]
fn input_validation_and_unknown_plan_do_not_confirm() {
    assert!(FusionClient::connect("http://user:pass@example.com", "token", None, 1.0).is_err());
    assert!(FusionClient::connect("https://example.com", "token", None, f64::NAN).is_err());
    assert!(task_path("..", "").is_err());
    let file = tempfile::NamedTempFile::new().unwrap();
    let (host, server) = server(vec![
        ok(json!({})),
        ok(json!({"id":"t"})),
        ok(json!([{ "id":"valid" }])),
    ]);
    let mut opt = options(file.path());
    opt.plan_id = Some("invalid".into());
    let error = client(&host).submit(&opt).unwrap_err();
    assert_eq!(error.data.unwrap()["job"]["id"], "t");
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
fn query_is_encoded_and_wait_returns_terminal_failure() {
    let (host, server) = server(vec![ok(json!([])), ok(json!({"id":"t","status":"failed"}))]);
    let c = client(&host);
    c.list_tasks(&[("name".into(), "a&b 中文".into())]).unwrap();
    assert_eq!(c.wait("t", 1.0, 0.01).unwrap()["status"], "failed");
    let requests = server.join().unwrap();
    assert!(requests[0].contains("name=a%26b"));
}

#[test]
fn recommendation_failure_retains_saved_task() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let (host, server) = server(vec![
        ok(json!({"package_id":"p"})),
        ok(json!({"task_id":"saved"})),
        (
            503,
            String::new(),
            json!({"code":503,"data":{"retry_later":true}}).to_string(),
        ),
    ]);
    let error = client(&host).submit(&options(file.path())).unwrap_err();
    let data = error.data.unwrap();
    assert_eq!(data["job"]["task_id"], "saved");
    assert_eq!(data["server_data"]["retry_later"], true);
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
fn retry_backoff_respects_wait_deadline() {
    let (host, server) = server(vec![(503, "Retry-After: 30\r\n".into(), String::new())]);
    let start = Instant::now();
    let error = client(&host).wait("t", 0.1, 0.01).unwrap_err();
    assert_eq!(error.kind, "timeout");
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn download_business_error_does_not_replace_result() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("result.zip");
    std::fs::write(&path, "old").unwrap();
    let (host, server) = server(vec![(200,
        "Content-Type: application/json\r\nContent-Disposition: attachment; filename=result.zip\r\n".into(),
        json!({"code":409,"msg":"not ready"}).to_string())]);
    let error = client(&host)
        .download_results("t", directory.path())
        .unwrap_err();
    assert_eq!(error.code, Some(json!(409)));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "old");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    server.join().unwrap();
}

#[test]
fn source_compatible_upload_and_empty_recommendation() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let (host, server) = server(vec![ok(json!("uploaded")), ok(json!({"plans":null}))]);
    let c = client(&host);
    let upload = c.upload_package(file.path()).unwrap();
    assert_eq!(
        upload["package_name"],
        file.path().file_name().unwrap().to_str().unwrap()
    );
    assert!(
        c.recommend("draft", "balanced", None, None)
            .unwrap()
            .is_empty()
    );
    assert_eq!(server.join().unwrap().len(), 2);
}
