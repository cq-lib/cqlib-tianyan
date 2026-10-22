//! Fusion cloud API. Public submission uses the deployed member v2 contract.
//! Deprecated SDK endpoints are retained for compatibility; the platform does not deploy them.
mod decision;
mod package;
mod transport;
mod v2;
pub use decision::{FusionCandidate, FusionDecision, FusionSchedule};
use transport::Body;

use crate::{
    TianyanError,
    auth::{self, Credentials},
    client::TianyanClient,
    config::TianyanConfig,
};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const API_PREFIX: &str = "/qccp-quantum/sdk/fusion";

/// Structured errors preserve server business codes and recovery data.
#[derive(Debug, thiserror::Error, Serialize)]
#[error("{kind}: {message}")]
pub struct FusionError {
    pub kind: String,
    pub message: String,
    pub http_status: Option<u16>,
    pub code: Option<Value>,
    pub data: Option<Value>,
}

impl FusionError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            kind: "invalid_input".into(),
            message: message.into(),
            http_status: None,
            code: None,
            data: None,
        }
    }
    fn response(status: u16, payload: &Value, fallback: &str) -> Self {
        Self {
            kind: "api".into(),
            message: payload
                .get("message")
                .or_else(|| payload.get("msg"))
                .or_else(|| payload.get("error"))
                .and_then(Value::as_str)
                .unwrap_or(fallback)
                .to_owned(),
            http_status: Some(status),
            code: payload.get("code").cloned(),
            data: payload.get("data").cloned(),
        }
    }
}
impl From<TianyanError> for FusionError {
    fn from(error: TianyanError) -> Self {
        if let TianyanError::Http(error) = error {
            return error.into();
        }
        Self {
            kind: "platform".into(),
            message: error.to_string(),
            http_status: None,
            code: None,
            data: None,
        }
    }
}
impl From<reqwest::Error> for FusionError {
    fn from(error: reqwest::Error) -> Self {
        Self {
            kind: if error.is_timeout() {
                "timeout"
            } else {
                "transport"
            }
            .into(),
            message: error.to_string(),
            http_status: error.status().map(|s| s.as_u16()),
            code: None,
            data: None,
        }
    }
}
impl From<std::io::Error> for FusionError {
    fn from(error: std::io::Error) -> Self {
        Self {
            kind: "io".into(),
            ..Self::invalid(error.to_string())
        }
    }
}
impl From<serde_json::Error> for FusionError {
    fn from(error: serde_json::Error) -> Self {
        Self {
            kind: "decode".into(),
            ..Self::invalid(error.to_string())
        }
    }
}

fn intelligent() -> String {
    "intelligent".into()
}
fn balanced() -> String {
    "balanced".into()
}

/// Options shared by Rust, Python and the C JSON interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitOptions {
    pub name: String,
    pub package: PathBuf,
    #[serde(default = "intelligent")]
    pub dispatch_mode: String,
    #[serde(default = "balanced")]
    pub dispatch_preference: String,
    #[serde(default)]
    pub quantum_resource: Option<String>,
    #[serde(default)]
    pub hpc_resource: Option<String>,
    #[serde(default)]
    pub auto_confirm: bool,
    #[serde(default)]
    pub plan_id: Option<String>,
}

/// Awaiting confirmation is a normal outcome, not a failed submission.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitOutcome {
    pub awaiting_confirmation: bool,
    pub job: Value,
    pub plans: Vec<Value>,
}

/// Cloneable handle sharing authentication with a TianyanPlatform.
#[derive(Clone)]
pub struct FusionClient {
    client: Arc<TianyanClient>,
    timeout: Duration,
}

#[allow(deprecated)]
impl FusionClient {
    pub fn from_client(client: Arc<TianyanClient>) -> Self {
        Self {
            client,
            timeout: Duration::from_secs(120),
        }
    }

    /// Use an API key, existing token, or both. Tokens and API keys are never
    /// persisted by this constructor. Prefer platform.fusion() to reuse login.
    pub fn connect(
        host: &str,
        token: &str,
        api_key: Option<&str>,
        timeout_secs: f64,
    ) -> Result<Self, FusionError> {
        let timeout = duration(timeout_secs, "timeout")?;
        let url = reqwest::Url::parse(host).map_err(|e| FusionError::invalid(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(FusionError::invalid(
                "host must be an HTTP(S) origin without credentials, path or query",
            ));
        }
        let origin = url.origin().ascii_serialization();
        let mut config = TianyanConfig::custom(origin.split_once("://").unwrap().1)
            .with_save_credentials(false)
            .with_auto_refresh(api_key.is_some_and(|k| !k.is_empty()));
        config.scheme = url.scheme().into();
        let creds = if token.is_empty() {
            let key = api_key
                .filter(|k| !k.is_empty())
                .ok_or_else(|| FusionError::invalid("API key or token is required"))?;
            auth::login_with_timeout(key, &config, timeout)?
        } else {
            Credentials {
                api_key: api_key.unwrap_or("").into(),
                access_token: token.into(),
                token_obtained_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            }
        };
        Ok(Self {
            client: Arc::new(TianyanClient::new(config, creds)?),
            timeout,
        })
    }

    pub fn with_timeout(mut self, seconds: f64) -> Result<Self, FusionError> {
        self.timeout = duration(seconds, "timeout")?;
        Ok(self)
    }

    pub fn access_token(&self) -> Result<String, FusionError> {
        Ok(self.client.credentials()?.access_token)
    }

    /// Explicit login updates the shared client's credential store.
    pub fn login(&self, api_key: &str) -> Result<String, FusionError> {
        Ok(self.client.login_with_key_timeout(api_key, self.timeout)?)
    }

    fn json(
        &self,
        method: Method,
        suffix: &str,
        query: &[(String, String)],
        body: Body<'_>,
    ) -> Result<Value, FusionError> {
        let response = self.client.fusion_request(
            method,
            &format!("{API_PREFIX}{suffix}"),
            query,
            body,
            self.timeout,
        )?;
        if response.status().as_u16() == 204 {
            return Ok(Value::Null);
        }
        let status = response.status().as_u16();
        let text = response.text()?;
        if text.is_empty() {
            return Ok(Value::Null);
        }
        let payload: Value = serde_json::from_str(&text)?;
        if let Some(code) = payload.get("code").filter(|v| !v.is_null()) {
            if code.as_i64() != Some(0) {
                return Err(FusionError::response(
                    status,
                    &payload,
                    "fusion API returned an error",
                ));
            }
        }
        Ok(payload.get("data").cloned().unwrap_or(payload))
    }

    /// Deprecated SDK compatibility API; use schedule_package for v2 uploads.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use schedule_package")]
    pub fn upload_package(&self, package: &Path) -> Result<Value, FusionError> {
        self.upload_package_legacy(package)
    }

    /// Deprecated SDK compatibility API; use schedule_package and submit_decision.
    #[deprecated(
        note = "platform does not deploy /sdk/fusion; use schedule_package and submit_decision"
    )]
    pub fn submit(&self, options: &SubmitOptions) -> Result<SubmitOutcome, FusionError> {
        self.submit_legacy(options)
    }

    /// Deprecated SDK compatibility API; use get_job or progress for v2 jobs.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use get_job or progress")]
    pub fn get(&self, task_id: &str) -> Result<Value, FusionError> {
        self.get_legacy(task_id)
    }

    /// Deprecated SDK compatibility API; use delete_job for v2 jobs.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use delete_job")]
    pub fn delete(&self, task_id: &str) -> Result<(), FusionError> {
        self.delete_legacy(task_id)
    }

    /// Deprecated SDK compatibility API; use wait_job for v2 jobs.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use wait_job")]
    pub fn wait(
        &self,
        task_id: &str,
        timeout_secs: f64,
        poll_interval: f64,
    ) -> Result<Value, FusionError> {
        self.wait_legacy(task_id, timeout_secs, poll_interval)
    }

    /// Deprecated SDK compatibility API; use download_job_result for v2 jobs.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use download_job_result")]
    pub fn download_results(&self, task_id: &str, dest: &Path) -> Result<PathBuf, FusionError> {
        self.download_results_legacy(task_id, dest)
    }

    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn upload_package_legacy(&self, package: &Path) -> Result<Value, FusionError> {
        package::validate(package)?;
        let data = self.json(Method::POST, "/package/upload", &[], Body::Upload(package))?;
        if data.is_object() {
            Ok(data)
        } else {
            Ok(json!({"package_name": package.file_name().unwrap_or_default().to_string_lossy()}))
        }
    }

    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn save(&self, options: &SubmitOptions) -> Result<Value, FusionError> {
        validate_options(options)?;
        let upload = self.upload_package_legacy(&options.package)?;
        let body = json!({"name": options.name, "package_name": package_name(&upload, &options.package),
            "package_id": upload.get("package_id"), "dispatch_mode": options.dispatch_mode,
            "dispatch_preference": options.dispatch_preference,
            "resource_hints": {"quantum": options.quantum_resource, "hpc": options.hpc_resource}, "action": "save"});
        let data = self.json(Method::POST, "/task/save", &[], Body::Json(&body))?;
        normalize_job(
            data,
            &json!({"name": options.name, "status": "wait", "dispatch_mode": options.dispatch_mode,
            "dispatch_preference": options.dispatch_preference}),
        )
    }

    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn recommend(
        &self,
        task_id: &str,
        preference: &str,
        quantum_hint: Option<&str>,
        hpc_hint: Option<&str>,
    ) -> Result<Vec<Value>, FusionError> {
        nonempty(task_id, "task_id")?;
        validate_preference(preference)?;
        let body = json!({"task_id":task_id,"dispatch_preference":preference,"resource_hints":{"quantum":quantum_hint,"hpc":hpc_hint}});
        let data = self.json(Method::POST, "/task/recommend", &[], Body::Json(&body))?;
        rows(data, "plans")
    }

    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn confirm(&self, task_id: &str, plan_id: &str) -> Result<Value, FusionError> {
        nonempty(task_id, "task_id")?;
        nonempty(plan_id, "plan_id")?;
        let body = json!({"task_id":task_id,"plan_id":plan_id});
        let data = self.json(Method::POST, "/task/confirm", &[], Body::Json(&body))?;
        normalize_job(data, &json!({"task_id":task_id,"status":"queued"}))
    }

    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn submit_legacy(&self, options: &SubmitOptions) -> Result<SubmitOutcome, FusionError> {
        validate_options(options)?;
        if options.dispatch_mode == "manual" {
            if options
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
                    .is_empty()
            {
                return Err(FusionError::invalid(
                    "manual dispatch requires quantum_resource and hpc_resource",
                ));
            }
            let upload = self.upload_package_legacy(&options.package)?;
            let body = json!({"name": options.name, "package_name": package_name(&upload, &options.package),
                "package_id": upload.get("package_id"), "dispatch_mode":"manual",
                "quantum_resource":options.quantum_resource,"hpc_resource":options.hpc_resource});
            let data = self.json(Method::POST, "/task/run", &[], Body::Json(&body))?;
            let job = normalize_job(
                data,
                &json!({"name":options.name,"status":"running","dispatch_mode":"manual",
                "quantum_resource":options.quantum_resource,"hpc_resource":options.hpc_resource}),
            )?;
            return Ok(SubmitOutcome {
                awaiting_confirmation: false,
                job,
                plans: vec![],
            });
        }
        let mut job = self.save(options)?;
        let task_id = job_id(&job)?;
        let plans = self
            .recommend(
                &task_id,
                &options.dispatch_preference,
                options.quantum_resource.as_deref(),
                options.hpc_resource.as_deref(),
            )
            .map_err(|mut error| {
                error.data = Some(json!({"job":job,"server_data":error.data}));
                error
            })?;
        let selected = if let Some(id) = &options.plan_id {
            let plan = plans
                .iter()
                .find(|p| identifier(p.get("plan_id").or_else(|| p.get("id"))) == *id)
                .ok_or_else(|| FusionError {
                    data: Some(json!({"job":job,"plans":plans})),
                    ..FusionError::invalid("unknown plan_id; draft remains awaiting confirmation")
                })?;
            Some(identifier(plan.get("plan_id").or_else(|| plan.get("id"))))
        } else if options.auto_confirm {
            let plan = plans.first().ok_or_else(|| FusionError {
                data: Some(json!({"job":job,"plans":plans})),
                ..FusionError::invalid(
                    "auto_confirm requested but recommendation returned no plans",
                )
            })?;
            Some(identifier(plan.get("plan_id").or_else(|| plan.get("id"))))
        } else {
            None
        };
        if let Some(plan_id) = selected {
            let job = self.confirm(&task_id, &plan_id).map_err(|mut error| {
                error.data = Some(
                    json!({"job":job,"plans":plans,"plan_id":plan_id,"server_data":error.data}),
                );
                error
            })?;
            Ok(SubmitOutcome {
                awaiting_confirmation: false,
                job,
                plans,
            })
        } else {
            job["status"] = json!("awaiting_confirm");
            Ok(SubmitOutcome {
                awaiting_confirmation: true,
                job,
                plans,
            })
        }
    }

    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn list_tasks(&self, query: &[(String, String)]) -> Result<Vec<Value>, FusionError> {
        rows(
            self.json(Method::GET, "/task/list", query, Body::Empty)?,
            "items",
        )
    }
    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn get_legacy(&self, task_id: &str) -> Result<Value, FusionError> {
        let data = self.json(Method::GET, &task_path(task_id, "")?, &[], Body::Empty)?;
        if !data.is_object() {
            return Err(FusionError::invalid("expected a task object"));
        }
        Ok(data)
    }
    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn events(&self, task_id: &str) -> Result<Vec<Value>, FusionError> {
        rows(
            self.json(
                Method::GET,
                &task_path(task_id, "/events")?,
                &[],
                Body::Empty,
            )?,
            "events",
        )
    }
    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn subtasks(&self, task_id: &str) -> Result<Vec<Value>, FusionError> {
        rows(
            self.json(
                Method::GET,
                &task_path(task_id, "/subtasks")?,
                &[],
                Body::Empty,
            )?,
            "items",
        )
    }
    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn delete_legacy(&self, task_id: &str) -> Result<(), FusionError> {
        self.json(Method::DELETE, &task_path(task_id, "")?, &[], Body::Empty)?;
        Ok(())
    }
    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn wait_legacy(
        &self,
        task_id: &str,
        timeout_secs: f64,
        poll_interval: f64,
    ) -> Result<Value, FusionError> {
        let timeout = duration(timeout_secs, "timeout")?;
        let interval = duration(poll_interval, "poll_interval")?;
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(FusionError {
                    kind: "timeout".into(),
                    ..FusionError::invalid("timeout waiting for fusion task")
                });
            }
            let mut client = self.clone();
            client.timeout = self.timeout.min(remaining);
            let job = client.get_legacy(task_id)?;
            if matches!(
                job.get("status").and_then(Value::as_str),
                Some("success" | "fail" | "failed" | "cancelled" | "error")
            ) {
                return Ok(job);
            }
            std::thread::sleep(interval.min(timeout.saturating_sub(started.elapsed())));
        }
    }

    /// Stream to a unique temporary file in the destination directory; replace
    /// the final file only after the complete response has been received.
    /// Legacy /sdk/fusion endpoint; not deployed by the platform. Use the v2 job API.
    #[deprecated(note = "platform does not deploy /sdk/fusion; use the v2 job API")]
    pub fn download_results_legacy(
        &self,
        task_id: &str,
        dest: &Path,
    ) -> Result<PathBuf, FusionError> {
        let path = task_path(task_id, "/results")?;
        let mut response = self.client.fusion_request(
            Method::GET,
            &format!("{API_PREFIX}{path}"),
            &[],
            Body::Empty,
            self.timeout,
        )?;
        let json_response = response
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("application/json")
            });
        let header = response
            .headers()
            .get("Content-Disposition")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let filename = download_name(header).unwrap_or_else(|| {
            format!(
                "fusion-{}-results.zip",
                safe_basename(task_id).unwrap_or_else(|| "task".into())
            )
        });
        std::fs::create_dir_all(dest)?;
        let target = dest.join(filename);
        let mut temporary = tempfile::NamedTempFile::new_in(dest)?;
        std::io::copy(&mut response, &mut temporary)?;
        if json_response {
            if let Ok(payload) = serde_json::from_reader::<_, Value>(temporary.reopen()?) {
                if payload
                    .get("code")
                    .is_some_and(|code| !code.is_null() && code.as_i64() != Some(0))
                {
                    return Err(FusionError::response(
                        200,
                        &payload,
                        "result download failed",
                    ));
                }
            }
        }
        temporary.as_file().sync_all()?;
        temporary.persist(&target).map_err(|e| e.error)?;
        Ok(target)
    }
}

fn duration(seconds: f64, field: &str) -> Result<Duration, FusionError> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(FusionError::invalid(format!(
            "{field} must be finite and positive"
        )));
    }
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| FusionError::invalid(format!("{field} is out of range")))
}
fn nonempty(value: &str, field: &str) -> Result<(), FusionError> {
    if value.trim().is_empty() {
        Err(FusionError::invalid(format!("{field} is required")))
    } else {
        Ok(())
    }
}
fn validate_preference(value: &str) -> Result<(), FusionError> {
    if !matches!(value, "quality" | "speed" | "cost" | "balanced") {
        return Err(FusionError::invalid("unknown dispatch_preference"));
    }
    Ok(())
}
fn validate_options(options: &SubmitOptions) -> Result<(), FusionError> {
    nonempty(&options.name, "name")?;
    if !matches!(options.dispatch_mode.as_str(), "manual" | "intelligent") {
        return Err(FusionError::invalid("unknown dispatch_mode"));
    }
    validate_preference(&options.dispatch_preference)
}
fn identifier(value: Option<&Value>) -> String {
    value
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_owned()
            } else if v.is_number() {
                v.to_string()
            } else {
                String::new()
            }
        })
        .unwrap_or_default()
}
fn job_id(job: &Value) -> Result<String, FusionError> {
    let id = identifier(job.get("task_id").or_else(|| job.get("id")));
    nonempty(&id, "task_id in response")?;
    Ok(id)
}
fn normalize_job(mut data: Value, defaults: &Value) -> Result<Value, FusionError> {
    if data.is_string() || data.is_number() {
        data = json!({"task_id":identifier(Some(&data))});
    }
    if data.is_null() && defaults.get("task_id").is_some() {
        data = json!({});
    }
    let object = data
        .as_object_mut()
        .ok_or_else(|| FusionError::invalid("expected task object or task ID"))?;
    for (key, value) in defaults.as_object().unwrap() {
        object.entry(key.clone()).or_insert_with(|| value.clone());
    }
    job_id(&data)?;
    Ok(data)
}
fn rows(data: Value, key: &str) -> Result<Vec<Value>, FusionError> {
    if data.is_null() {
        return Ok(vec![]);
    }
    if let Some(object) = data.as_object() {
        if object.get(key).is_none_or(Value::is_null)
            && object.get("items").is_none_or(Value::is_null)
        {
            return Ok(vec![]);
        }
    }
    data.as_array()
        .or_else(|| data.get(key).and_then(Value::as_array))
        .or_else(|| data.get("items").and_then(Value::as_array))
        .cloned()
        .ok_or_else(|| FusionError::invalid(format!("expected array or {key} list")))
}
fn package_name(upload: &Value, path: &Path) -> String {
    upload
        .get("package_name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}
fn task_path(id: &str, suffix: &str) -> Result<String, FusionError> {
    nonempty(id, "task_id")?;
    if id == "." || id == ".." {
        return Err(FusionError::invalid("invalid task_id"));
    }
    Ok(format!(
        "/task/{}{suffix}",
        percent_encoding::utf8_percent_encode(id, percent_encoding::NON_ALPHANUMERIC)
    ))
}
fn safe_basename(name: &str) -> Option<String> {
    let normalized = name.replace('\\', "/");
    let name = normalized.rsplit('/').next()?.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return None;
    }
    Some(name.to_owned())
}
fn download_name(header: &str) -> Option<String> {
    let mut fallback = None;
    for item in header.split(';') {
        if let Some((key, value)) = item.trim().split_once('=') {
            let value = value.trim().trim_matches('"');
            if key.eq_ignore_ascii_case("filename*") {
                let parts: Vec<_> = value.splitn(3, '\'').collect();
                if parts.len() == 3 && parts[0].eq_ignore_ascii_case("UTF-8") {
                    if let Ok(decoded) =
                        percent_encoding::percent_decode_str(parts[2]).decode_utf8()
                    {
                        if let Some(name) = safe_basename(&decoded) {
                            return Some(name);
                        }
                    }
                }
            } else if key.eq_ignore_ascii_case("filename") {
                fallback = safe_basename(value);
            }
        }
    }
    fallback
}

#[cfg(test)]
mod tests;
