//! Member hybrid-job API used by `fusion_schedule` and `fusion_submit`.
use super::decision::{
    FusionCandidate, FusionDecision, FusionSchedule, ResourceChoice, ResourceChoices, WaitSignal,
    annotate, select_manual, select_rank, top_candidates, wait_signal,
};
use super::transport::Body;
use super::{FusionClient, FusionError};
use reqwest::Method;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const JOBS: &str = "/qccp-hybrid/hybrid/job/v2";
const SCHEDULE: &str = "/qccp-quantum/fusion/strategy/schedule";

impl FusionClient {
    /// Upload the package, request recommendations, and load resource codes.
    pub fn schedule_package(
        &self,
        package: &Path,
        strategy: &str,
    ) -> Result<FusionSchedule, FusionError> {
        if !package.is_file() {
            return Err(FusionError::invalid("package must be an existing file"));
        }
        let strategy = schedule_strategy(strategy)?;
        let uploaded = self.member(Method::POST, &format!("{JOBS}/upload"), &[], Body::Upload(package))?;
        let file_id = uploaded
            .get("fileId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| FusionError::invalid("upload response is missing fileId"))?
            .to_owned();
        let body = json!({"fileId": file_id, "strategy": strategy});
        let assessed = self.member(Method::POST, SCHEDULE, &[("refresh".into(), "false".into()), ("details".into(), "false".into())], Body::Json(&body))?;
        let choices = self.resource_choices()?;
        let recommended = assessed
            .get("recommended_group_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let candidates = if recommended.is_none() {
            Vec::new()
        } else {
            let ranked = assessed
                .get("ranked_candidates")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            top_candidates(
                ranked
                    .iter()
                    .enumerate()
                    .map(|(index, item)| candidate_from_json(item, index, &choices))
                    .collect(),
            )
        };
        Ok(FusionSchedule {
            file_id,
            candidates,
            choices,
        })
    }

    pub fn submit_decision(
        &self,
        name: &str,
        decision: &FusionDecision,
    ) -> Result<Value, FusionError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 50 {
            return Err(FusionError::invalid("task name must be 1 to 50 characters"));
        }
        let request_id = client_request_id();
        let mut body = json!({
            "taskName": name,
            "fileId": decision.file_id,
            "qpuResourceCode": decision.qpu_code,
            "cpuResourceCode": decision.cpu_code,
            "schedulingMode": decision.scheduling_mode,
            "clientRequestId": request_id,
        });
        if decision.scheduling_mode == "SMART" {
            if let Some(preference) = &decision.scheduling_preference {
                body["schedulingPreference"] = json!(preference);
            }
        }
        let data = self.member_allowing_unconfirmed(
            Method::POST,
            &format!("{JOBS}/submit"),
            Body::Json(&body),
        )?;
        if let Some(id) = unconfirmed_id(&data) {
            return self.member(
                Method::POST,
                &format!("{JOBS}/tasks/{id}/retry"),
                &[],
                Body::Json(&body),
            );
        }
        Ok(data)
    }

    pub fn wait_job(
        &self,
        task_id: &str,
        timeout_secs: f64,
        poll_interval: f64,
    ) -> Result<Value, FusionError> {
        let timeout = positive_seconds(timeout_secs, "timeout")?;
        let interval = positive_seconds(poll_interval, "poll_interval")?;
        let started = Instant::now();
        loop {
            if started.elapsed() >= timeout {
                return Err(FusionError {
                    kind: "timeout".into(),
                    ..FusionError::invalid("timeout waiting for fusion task")
                });
            }
            let task = self.member(Method::GET, &format!("{JOBS}/tasks/{task_id}"), &[], Body::Empty)?;
            let status = task
                .get("status")
                .and_then(Value::as_i64)
                .ok_or_else(|| FusionError::invalid("task status is missing"))?;
            if wait_signal(status)? == WaitSignal::Ready {
                return Ok(task);
            }
            std::thread::sleep(interval.min(timeout.saturating_sub(started.elapsed())));
        }
    }

    pub fn download_job_result(&self, task_id: &str, dest: &Path) -> Result<PathBuf, FusionError> {
        let mut response = self.client.fusion_request(
            Method::GET,
            &format!("{JOBS}/tasks/{task_id}/result"),
            &[],
            Body::Empty,
            self.timeout,
        )?;
        let status = response.status().as_u16();
        let json_response = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("application/json"));
        if status >= 400 || json_response {
            let text = response.text().unwrap_or_default();
            let payload = serde_json::from_str(&text).unwrap_or(Value::Null);
            return Err(FusionError::response(
                status,
                &payload,
                "result file is not available",
            ));
        }
        std::fs::create_dir_all(dest)?;
        let target = dest.join(format!("fusion-{task_id}-result"));
        let mut temporary = tempfile::NamedTempFile::new_in(dest)?;
        std::io::copy(&mut response, &mut temporary)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&target).map_err(|error| error.error)?;
        Ok(target)
    }

    fn resource_choices(&self) -> Result<ResourceChoices, FusionError> {
        let mut qpu = Vec::new();
        let mut cpu = Vec::new();
        let mut page = 1u64;
        loop {
            let data = self.member(
                Method::GET,
                &format!("{JOBS}/resources/choices"),
                &[
                    ("pageNo".into(), page.to_string()),
                    ("pageSize".into(), "100".into()),
                ],
                Body::Empty,
            )?;
            qpu.extend(choice_page(data.get("qpu")));
            cpu.extend(choice_page(data.get("cpu")));
            let qpu_total = data.get("qpuTotal").and_then(Value::as_u64).unwrap_or(qpu.len() as u64);
            let cpu_total = data.get("cpuTotal").and_then(Value::as_u64).unwrap_or(cpu.len() as u64);
            if qpu.len() as u64 >= qpu_total && cpu.len() as u64 >= cpu_total {
                break;
            }
            page += 1;
            if page > 100 {
                break;
            }
        }
        Ok(ResourceChoices { qpu, cpu })
    }

    fn member_allowing_unconfirmed(
        &self,
        method: Method,
        path: &str,
        body: Body<'_>,
    ) -> Result<Value, FusionError> {
        let (status, text) = self.exchange(method, path, &[], body)?;
        let payload: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if unconfirmed_id(payload.get("data").unwrap_or(&payload)).is_some() {
            return Ok(payload.get("data").cloned().unwrap_or(payload));
        }
        match payload.get("code").and_then(Value::as_i64) {
            Some(200) => Ok(payload.get("data").cloned().unwrap_or(Value::Null)),
            Some(_) => Err(FusionError::response(status, &payload, "fusion request failed")),
            None if status >= 400 => Err(FusionError {
                http_status: Some(status),
                ..FusionError::invalid("fusion request failed")
            }),
            None => Ok(payload),
        }
    }

    fn member(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Body<'_>,
    ) -> Result<Value, FusionError> {
        let (status, text) = self.exchange(method, path, query, body)?;
        if text.is_empty() {
            return if status >= 400 {
                Err(FusionError::invalid("fusion request failed"))
            } else {
                Ok(Value::Null)
            };
        }
        let payload: Value = serde_json::from_str(&text)?;
        match payload.get("code").and_then(Value::as_i64) {
            Some(200) => Ok(payload.get("data").cloned().unwrap_or(Value::Null)),
            Some(_) => Err(FusionError::response(status, &payload, "fusion request failed")),
            None if status >= 400 => {
                let message = payload
                    .get("error")
                    .or_else(|| payload.get("msg"))
                    .and_then(Value::as_str)
                    .unwrap_or("fusion request failed");
                Err(FusionError {
                    http_status: Some(status),
                    ..FusionError::invalid(message)
                })
            }
            None => Ok(payload),
        }
    }

    fn exchange(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Body<'_>,
    ) -> Result<(u16, String), FusionError> {
        let response = self
            .client
            .fusion_request(method, path, query, body, self.timeout)?;
        let status = response.status().as_u16();
        let text = response.text()?;
        Ok((status, text))
    }
}

impl FusionSchedule {
    pub fn select(&self, rank: i64) -> Result<FusionDecision, FusionError> {
        select_rank(self, rank)
    }

    pub fn manual(&self, qpu: &str, cpu: &str) -> Result<FusionDecision, FusionError> {
        select_manual(self, qpu, cpu)
    }
}

fn candidate_from_json(value: &Value, index: usize, choices: &ResourceChoices) -> FusionCandidate {
    let device_id = value
        .pointer("/qpu/device_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let resource_id = value
        .pointer("/classic/resource_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let meets = value
        .get("meets_all_constraints")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let (qpu_code, cpu_code, selectable, reject_reason) =
        annotate(&device_id, &resource_id, meets, choices);
    FusionCandidate {
        rank: value.get("rank").and_then(Value::as_i64).unwrap_or(index as i64 + 1),
        group_id: value.get("group_id").and_then(Value::as_str).unwrap_or("").to_owned(),
        device_id,
        resource_id,
        qpu_code,
        cpu_code,
        meets_all_constraints: meets,
        scores: value.get("scores").cloned().unwrap_or(Value::Null),
        selectable,
        reject_reason,
    }
}

fn choice_page(value: Option<&Value>) -> Vec<ResourceChoice> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let code = item.get("code")?.as_str()?.to_owned();
                    if code.is_empty() {
                        return None;
                    }
                    Some(ResourceChoice {
                        code,
                        available: item.get("available").and_then(Value::as_bool).unwrap_or(true),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn unconfirmed_id(data: &Value) -> Option<String> {
    let unconfirmed = data
        .get("submissionState")
        .and_then(Value::as_str)
        .is_some_and(|state| state.eq_ignore_ascii_case("UNCONFIRMED"))
        || data
            .get("errorCode")
            .and_then(Value::as_str)
            .is_some_and(|code| code == "FUSION_QRMI_UNCONFIRMED");
    if !unconfirmed {
        return None;
    }
    data.get("id")
        .or_else(|| data.get("rootId"))
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn schedule_strategy(value: &str) -> Result<String, FusionError> {
    Ok(match value {
        "balanced" | "BALANCED" => "balanced".into(),
        "quality" | "quality_first" | "QUALITY_FIRST" => "quality_first".into(),
        "speed" | "speed_first" | "SPEED_FIRST" => "speed_first".into(),
        "cost" | "cost_first" | "COST_FIRST" => "cost_first".into(),
        _ => return Err(FusionError::invalid("unknown scheduling strategy")),
    })
}

fn client_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("sdk-{nanos:x}")
}

fn positive_seconds(seconds: f64, field: &str) -> Result<Duration, FusionError> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(FusionError::invalid(format!(
            "{field} must be finite and positive"
        )));
    }
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| FusionError::invalid(format!("{field} is out of range")))
}
