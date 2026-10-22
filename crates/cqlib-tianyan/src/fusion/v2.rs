//! Member hybrid-job API used by `fusion_schedule` and `fusion_submit`.
use super::decision::{
    FusionCandidate, FusionDecision, FusionSchedule, ResourceChoice, ResourceChoices, WaitSignal,
    annotate, select_manual, select_rank, top_candidates, wait_signal,
};
use super::transport::Body;
use super::{FusionClient, FusionError, SubmitOptions, SubmitOutcome};
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
        let strategy = schedule_strategy(strategy)?;
        let uploaded = self.upload_job_package(package)?;
        let file_id = uploaded
            .get("fileId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| FusionError::invalid("upload response is missing fileId"))?
            .to_owned();
        let body = json!({"fileId": file_id, "strategy": strategy});
        let assessed = self.member(
            Method::POST,
            SCHEDULE,
            &[
                ("refresh".into(), "false".into()),
                ("details".into(), "false".into()),
            ],
            Body::Json(&body),
        )?;
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
        super::nonempty(&decision.file_id, "fileId")?;
        super::nonempty(&decision.qpu_code, "qpuResourceCode")?;
        super::nonempty(&decision.cpu_code, "cpuResourceCode")?;
        if !matches!(decision.scheduling_mode.as_str(), "MANUAL" | "SMART") {
            return Err(FusionError::invalid(
                "schedulingMode must be MANUAL or SMART",
            ));
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
                &job_path(&id, "/retry")?,
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
            let mut client = self.clone();
            client.timeout = self.timeout.min(timeout.saturating_sub(started.elapsed()));
            let mut task = client.progress(task_id)?;
            let status = task
                .get("status")
                .and_then(Value::as_i64)
                .ok_or_else(|| FusionError::invalid("task status is missing"))?;
            if wait_signal(status)? == WaitSignal::Ready {
                // /progress has no ID; the high-level wait receipt keeps the requested ID.
                task["id"] = json!(task_id);
                return Ok(task);
            }
            std::thread::sleep(interval.min(timeout.saturating_sub(started.elapsed())));
        }
    }

    /// Download the JSON result as a file, preserving existing files on failure.
    pub fn download_job_result(&self, task_id: &str, dest: &Path) -> Result<PathBuf, FusionError> {
        self.download_job_file(task_id, "result", dest, "json")
    }

    pub fn download_job_log(&self, task_id: &str, dest: &Path) -> Result<PathBuf, FusionError> {
        self.download_job_file(task_id, "log", dest, "log")
    }

    fn download_job_file(
        &self,
        task_id: &str,
        kind: &str,
        dest: &Path,
        extension: &str,
    ) -> Result<PathBuf, FusionError> {
        let path = job_path(task_id, &format!("/{kind}"))?;
        let mut response =
            self.client
                .fusion_request(Method::GET, &path, &[], Body::Empty, self.timeout)?;
        let status = response.status().as_u16();
        if status >= 400 {
            let payload = response.json().unwrap_or(Value::Null);
            return Err(FusionError::response(
                status,
                &payload,
                "file is not available",
            ));
        }
        let filename = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|value| value.to_str().ok())
            .and_then(super::download_name)
            .unwrap_or_else(|| {
                format!(
                    "fusion-{}-{kind}.{extension}",
                    super::safe_basename(task_id).unwrap_or_else(|| "task".into())
                )
            });
        std::fs::create_dir_all(dest)?;
        let target = dest.join(filename);
        let mut temporary = tempfile::NamedTempFile::new_in(dest)?;
        std::io::copy(&mut response, &mut temporary)?;
        // Successful results are arbitrary JSON files, not response envelopes.
        // Download errors are identified by HTTP status as specified by v2.
        temporary.as_file().sync_all()?;
        temporary.persist(&target).map_err(|error| error.error)?;
        Ok(target)
    }

    /// Validated v2 multipart upload. No HTTP is sent for an invalid archive.
    pub fn upload_job_package(&self, package: &Path) -> Result<Value, FusionError> {
        super::package::validate(package)?;
        self.member(
            Method::POST,
            &format!("{JOBS}/upload"),
            &[],
            Body::Upload(package),
        )
    }

    /// Compatibility-shaped submission using the deployed v2 protocol.
    /// Without confirmation, returns recommendations; no task or draft is created.
    pub fn submit_job(&self, options: &SubmitOptions) -> Result<SubmitOutcome, FusionError> {
        super::validate_options(options)?;
        if options.name.trim().chars().count() > 50 {
            return Err(FusionError::invalid("task name must be 1 to 50 characters"));
        }
        if options.dispatch_mode == "manual"
            && (options
                .quantum_resource
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
                || options
                    .hpc_resource
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty())
        {
            return Err(FusionError::invalid(
                "manual dispatch requires quantum_resource and hpc_resource",
            ));
        }
        let schedule = if options.dispatch_mode == "manual" {
            let upload = self.upload_job_package(&options.package)?;
            let file_id = upload
                .get("fileId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| FusionError::invalid("upload response is missing fileId"))?
                .to_owned();
            FusionSchedule {
                file_id,
                candidates: vec![],
                choices: self.resource_choices()?,
            }
        } else {
            self.schedule_package(&options.package, &options.dispatch_preference)?
        };
        let plans: Vec<Value> = schedule
            .candidates
            .iter()
            .map(|candidate| {
                json!({
                    "plan_id": candidate.group_id, "rank": candidate.rank,
                    "quantum_resource": candidate.qpu_code, "hpc_resource": candidate.cpu_code,
                    "selectable": candidate.selectable, "reject_reason": candidate.reject_reason,
                    "fileId": schedule.file_id,
                })
            })
            .collect();
        let decision = if options.dispatch_mode == "manual" {
            Some(schedule.manual(
                options.quantum_resource.as_deref().unwrap(),
                options.hpc_resource.as_deref().unwrap(),
            )?)
        } else if let Some(plan_id) = &options.plan_id {
            let candidate = schedule
                .candidates
                .iter()
                .find(|item| item.group_id == *plan_id)
                .ok_or_else(|| {
                    FusionError::invalid(
                        "unknown plan_id; schedule again and choose a current recommendation",
                    )
                })?;
            Some(schedule.select(candidate.rank)?)
        } else if options.auto_confirm {
            let candidate = schedule
                .candidates
                .iter()
                .find(|item| item.selectable)
                .ok_or_else(|| {
                    FusionError::invalid(
                        "no selectable recommendation; choose available resource codes manually",
                    )
                })?;
            Some(schedule.select(candidate.rank)?)
        } else {
            None
        };
        match decision {
            Some(mut decision) => {
                if decision.scheduling_mode == "SMART" {
                    decision.scheduling_preference =
                        Some(schedule_strategy(&options.dispatch_preference)?.to_uppercase());
                }
                Ok(SubmitOutcome {
                    awaiting_confirmation: false,
                    job: self.submit_decision(&options.name, &decision)?,
                    plans,
                })
            }
            None => Ok(SubmitOutcome {
                awaiting_confirmation: true,
                job: json!({"fileId": schedule.file_id, "name": options.name, "status":"awaiting_confirm"}),
                plans,
            }),
        }
    }

    /// Page of parent jobs. Preserves total, pageNo, pageSize and records.
    pub fn list_jobs(
        &self,
        page_no: u32,
        page_size: u32,
        view: &str,
    ) -> Result<Value, FusionError> {
        if page_no == 0 || !(1..=100).contains(&page_size) || !matches!(view, "ALL" | "DRAFT") {
            return Err(FusionError::invalid(
                "pageNo must be positive, pageSize 1..100, view ALL or DRAFT",
            ));
        }
        self.member(
            Method::GET,
            &format!("{JOBS}/tasks"),
            &[
                ("pageNo".into(), page_no.to_string()),
                ("pageSize".into(), page_size.to_string()),
                ("view".into(), view.into()),
            ],
            Body::Empty,
        )
    }

    pub fn presets(&self) -> Result<Value, FusionError> {
        self.member(Method::GET, &format!("{JOBS}/presets"), &[], Body::Empty)
    }

    pub fn progress(&self, task_id: &str) -> Result<Value, FusionError> {
        self.member(
            Method::GET,
            &job_path(task_id, "/progress")?,
            &[],
            Body::Empty,
        )
    }

    pub fn get_job(&self, task_id: &str) -> Result<Value, FusionError> {
        self.member(Method::GET, &job_path(task_id, "")?, &[], Body::Empty)
    }

    /// Soft-delete a draft or confirmed terminal job; the server checks eligibility.
    pub fn delete_job(&self, task_id: &str) -> Result<Value, FusionError> {
        self.member(Method::DELETE, &job_path(task_id, "")?, &[], Body::Empty)
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
            let qpu_total = data
                .get("qpuTotal")
                .and_then(Value::as_u64)
                .unwrap_or(qpu.len() as u64);
            let cpu_total = data
                .get("cpuTotal")
                .and_then(Value::as_u64)
                .unwrap_or(cpu.len() as u64);
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
        member_payload(status, payload)
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
            return member_payload(status, Value::Null);
        }
        let payload: Value = serde_json::from_str(&text).map_err(|_| FusionError {
            http_status: Some(status),
            ..FusionError::invalid("fusion response is not valid JSON")
        })?;
        member_payload(status, payload)
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
        rank: value
            .get("rank")
            .and_then(Value::as_i64)
            .unwrap_or(index as i64 + 1),
        group_id: value
            .get("group_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
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
                        available: item
                            .get("available")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
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
        "unified" => "unified".into(),
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

fn job_path(id: &str, suffix: &str) -> Result<String, FusionError> {
    super::nonempty(id, "task_id")?;
    if matches!(id, "." | "..") {
        return Err(FusionError::invalid("invalid task_id"));
    }
    Ok(format!(
        "{JOBS}/tasks/{}{suffix}",
        percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC)
    ))
}

fn member_payload(status: u16, payload: Value) -> Result<Value, FusionError> {
    if status >= 400
        || payload
            .get("code")
            .is_some_and(|code| code.as_i64() != Some(200))
    {
        return Err(FusionError::response(
            status,
            &payload,
            "fusion request failed",
        ));
    }
    if payload.get("code").is_some() {
        Ok(payload.get("data").cloned().unwrap_or(Value::Null))
    } else {
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_without_available_are_selectable() {
        let choices = ResourceChoices {
            qpu: choice_page(Some(&json!([{ "code": "q" }]))),
            cpu: choice_page(Some(&json!([{ "code": "c" }]))),
        };
        let candidate = candidate_from_json(
            &json!({
                "qpu": { "device_id": "q" },
                "classic": { "resource_id": "c" },
                "meets_all_constraints": true,
            }),
            0,
            &choices,
        );
        assert!(candidate.selectable);
        assert!(candidate.reject_reason.is_none());
        assert!(!choice_page(Some(&json!([{ "code": "q", "available": false }])))[0].available);
    }
}
