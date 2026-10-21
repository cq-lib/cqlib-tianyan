//! Fusion C API. Handles own an Arc to the shared Rust client. Returned JSON
//! strings belong to the caller and must be released with tianyan_string_free.
use crate::{
    error::{clear_last_error, cstr_to_str, set_last_error, str_to_cstring},
    platform::TianyanPlatformC,
};
use cqlib_tianyan::fusion::{FusionClient, FusionError, SubmitOptions};
use serde_json::{Value, json};
use std::{
    os::raw::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

/// Opaque fusion handle. Free with tianyan_fusion_free, never C free().
pub struct TianyanFusionC(FusionClient);

fn failure(error: FusionError) {
    set_last_error(serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()));
}
fn text<'a>(value: *const c_char, name: &str) -> Result<&'a str, FusionError> {
    cstr_to_str(value, name).map_err(FusionError::invalid)
}
fn optional<'a>(value: *const c_char) -> Result<Option<&'a str>, FusionError> {
    if value.is_null() {
        Ok(None)
    } else {
        text(value, "optional string").map(Some)
    }
}
fn create(call: impl FnOnce() -> Result<FusionClient, FusionError>) -> *mut TianyanFusionC {
    clear_last_error();
    match catch_unwind(AssertUnwindSafe(call)) {
        Ok(Ok(client)) => Box::into_raw(Box::new(TianyanFusionC(client))),
        Ok(Err(error)) => {
            failure(error);
            std::ptr::null_mut()
        }
        Err(_) => {
            failure(FusionError::invalid("internal panic in fusion client"));
            std::ptr::null_mut()
        }
    }
}
fn call(
    client: *const TianyanFusionC,
    action: impl FnOnce(&FusionClient) -> Result<Value, FusionError>,
) -> *mut c_char {
    clear_last_error();
    if client.is_null() {
        failure(FusionError::invalid("fusion client must not be NULL"));
        return std::ptr::null_mut();
    }
    // SAFETY: callers keep this library-created handle alive for the call.
    match catch_unwind(AssertUnwindSafe(|| action(&unsafe { &*client }.0))) {
        Ok(Ok(value)) => str_to_cstring(&value.to_string()),
        Ok(Err(error)) => {
            failure(error);
            std::ptr::null_mut()
        }
        Err(_) => {
            failure(FusionError::invalid("internal panic in fusion client"));
            std::ptr::null_mut()
        }
    }
}

/// Shares platform credentials, including future refreshes. The platform may
/// be freed after this call; the fusion handle keeps the session alive.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_from_platform(
    platform: *const TianyanPlatformC,
) -> *mut TianyanFusionC {
    create(|| {
        if platform.is_null() {
            return Err(FusionError::invalid("platform must not be NULL"));
        }
        // SAFETY: caller supplies a live handle created by this library.
        Ok(unsafe { &*platform }.0.fusion())
    })
}

/// Connect with API key, token, or both (no credential persistence).
/// host must be an HTTP(S) origin. token/api_key may be NULL, but not both.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_connect(
    host: *const c_char,
    token: *const c_char,
    api_key: *const c_char,
    timeout_secs: f64,
) -> *mut TianyanFusionC {
    create(|| {
        FusionClient::connect(
            text(host, "host")?,
            optional(token)?.unwrap_or(""),
            optional(api_key)?,
            timeout_secs,
        )
    })
}

/// Release a fusion handle. NULL is a no-op. Do not free a handle while another
/// thread is using it, and never call this function twice for the same handle.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_free(client: *mut TianyanFusionC) {
    if !client.is_null() {
        // SAFETY: allocated with Box::into_raw above; caller transfers ownership.
        unsafe {
            drop(Box::from_raw(client));
        }
    }
}

/// Returns uploaded package metadata as JSON; package is a UTF-8 file path.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_upload(
    client: *const TianyanFusionC,
    package: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        c.upload_package(Path::new(text(package, "package")?))
    })
}

/// options_json is a SubmitOptions object: name, package, dispatch_mode,
/// dispatch_preference, quantum_resource, hpc_resource, auto_confirm, plan_id.
/// Save uploads the package and creates a draft; it never enqueues execution.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_save(
    client: *const TianyanFusionC,
    options_json: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        let options: SubmitOptions = serde_json::from_str(text(options_json, "options_json")?)?;
        c.save(&options)
    })
}

/// Returns {awaiting_confirmation, job, plans}. awaiting_confirmation=true is
/// successful draft creation, NOT an error and NOT an enqueued task.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_submit(
    client: *const TianyanFusionC,
    options_json: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        let options: SubmitOptions = serde_json::from_str(text(options_json, "options_json")?)?;
        Ok(serde_json::to_value(c.submit(&options)?)?)
    })
}

/// preference may be NULL (balanced); resource hints may be NULL.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_recommend(
    client: *const TianyanFusionC,
    task_id: *const c_char,
    preference: *const c_char,
    quantum_hint: *const c_char,
    hpc_hint: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        Ok(json!(c.recommend(
            text(task_id, "task_id")?,
            optional(preference)?.unwrap_or("balanced"),
            optional(quantum_hint)?,
            optional(hpc_hint)?
        )?))
    })
}

/// Confirm an existing recommendation and enqueue its task.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_confirm(
    client: *const TianyanFusionC,
    task_id: *const c_char,
    plan_id: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        c.confirm(text(task_id, "task_id")?, text(plan_id, "plan_id")?)
    })
}

/// query_json is an object of name/status/dispatch_mode/limit/offset. NULL
/// uses the server defaults. Returns a JSON array of task objects.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_list(
    client: *const TianyanFusionC,
    query_json: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        let query: Value = serde_json::from_str(optional(query_json)?.unwrap_or("{}"))?;
        let object = query
            .as_object()
            .ok_or_else(|| FusionError::invalid("query must be an object"))?;
        let query: Vec<_> = object
            .iter()
            .map(|(k, v)| {
                Ok((
                    k.clone(),
                    match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        _ => {
                            return Err(FusionError::invalid(
                                "query values must be strings or numbers",
                            ));
                        }
                    },
                ))
            })
            .collect::<Result<_, FusionError>>()?;
        Ok(json!(c.list_tasks(&query)?))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_get(
    client: *const TianyanFusionC,
    task_id: *const c_char,
) -> *mut c_char {
    call(client, |c| c.get(text(task_id, "task_id")?))
}
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_events(
    client: *const TianyanFusionC,
    task_id: *const c_char,
) -> *mut c_char {
    call(client, |c| Ok(json!(c.events(text(task_id, "task_id")?)?)))
}
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_subtasks(
    client: *const TianyanFusionC,
    task_id: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        Ok(json!(c.subtasks(text(task_id, "task_id")?)?))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_delete(
    client: *const TianyanFusionC,
    task_id: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        c.delete(text(task_id, "task_id")?)?;
        Ok(json!({"deleted":true}))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_wait(
    client: *const TianyanFusionC,
    task_id: *const c_char,
    timeout_secs: f64,
    poll_interval_secs: f64,
) -> *mut c_char {
    call(client, |c| {
        c.wait(text(task_id, "task_id")?, timeout_secs, poll_interval_secs)
    })
}

/// Downloads atomically to dest_dir and returns {"path":"..."}. On any
/// failure returns NULL; tianyan_last_error() contains structured error JSON.
#[unsafe(no_mangle)]
pub extern "C" fn tianyan_fusion_download(
    client: *const TianyanFusionC,
    task_id: *const c_char,
    dest_dir: *const c_char,
) -> *mut c_char {
    call(client, |c| {
        Ok(
            json!({"path":c.download_results(text(task_id, "task_id")?, Path::new(text(dest_dir, "dest_dir")?))?}),
        )
    })
}
