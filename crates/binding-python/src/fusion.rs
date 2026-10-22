//! Thin Python binding for the shared Rust fusion client. Wire models cross
//! the boundary as JSON; the Python facade exposes the existing dataclasses.
use cqlib_tianyan::fusion::{FusionClient, FusionError, SubmitOptions};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use serde::Serialize;
use std::path::Path;

pyo3::create_exception!(_cqlib_tianyan, FusionNativeError, PyRuntimeError);

pub(crate) fn py_error(error: FusionError) -> PyErr {
    FusionNativeError::new_err(serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))
}
fn json<T: Serialize>(result: Result<T, FusionError>) -> PyResult<String> {
    serde_json::to_string(&result.map_err(py_error)?).map_err(|e| py_error(e.into()))
}
fn options(value: &str) -> PyResult<SubmitOptions> {
    serde_json::from_str(value).map_err(|e| py_error(e.into()))
}

#[pyclass(name = "_FusionClient", module = "cqlib_tianyan._cqlib_tianyan")]
pub struct PyFusionClient {
    inner: FusionClient,
}

#[pymethods]
impl PyFusionClient {
    #[new]
    #[pyo3(signature = (host="https://qc.zdxlz.com", token="", api_key=None, timeout=120.0))]
    fn new(
        py: Python<'_>,
        host: &str,
        token: &str,
        api_key: Option<&str>,
        timeout: f64,
    ) -> PyResult<Self> {
        py.detach(|| FusionClient::connect(host, token, api_key, timeout))
            .map(|inner| Self { inner })
            .map_err(py_error)
    }

    #[staticmethod]
    #[pyo3(signature = (platform, timeout=120.0))]
    fn from_platform(
        platform: &crate::platform::PyTianyanPlatform,
        timeout: f64,
    ) -> PyResult<Self> {
        platform
            .inner
            .fusion()
            .with_timeout(timeout)
            .map(|inner| Self { inner })
            .map_err(py_error)
    }

    fn access_token(&self) -> PyResult<String> {
        self.inner.access_token().map_err(py_error)
    }

    fn login(&self, py: Python<'_>, api_key: &str) -> PyResult<String> {
        py.detach(|| self.inner.login(api_key)).map_err(py_error)
    }

    #[pyo3(signature = (package, strategy="balanced"))]
    fn schedule_package(
        &self,
        py: Python<'_>,
        package: &str,
        strategy: &str,
    ) -> PyResult<PyFusionSchedule> {
        py.detach(|| self.inner.schedule_package(Path::new(package), strategy))
            .map(|inner| PyFusionSchedule { inner })
            .map_err(py_error)
    }

    fn submit_decision(
        &self,
        py: Python<'_>,
        name: &str,
        decision: &PyFusionDecision,
    ) -> PyResult<PyFusionJob> {
        let task = py
            .detach(|| self.inner.submit_decision(name, &decision.inner))
            .map_err(py_error)?;
        let task_id = task
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| py_error(FusionError::invalid("submit response is missing id")))?
            .to_owned();
        Ok(PyFusionJob {
            client: self.inner.clone(),
            task_id,
            snapshot: task.to_string(),
        })
    }

    #[pyo3(signature = (page_no=1, page_size=20, view="ALL"))]
    fn list_jobs(
        &self,
        py: Python<'_>,
        page_no: u32,
        page_size: u32,
        view: &str,
    ) -> PyResult<String> {
        json(py.detach(|| self.inner.list_jobs(page_no, page_size, view)))
    }
    fn presets(&self, py: Python<'_>) -> PyResult<String> {
        json(py.detach(|| self.inner.presets()))
    }
    fn progress(&self, py: Python<'_>, task_id: &str) -> PyResult<String> {
        json(py.detach(|| self.inner.progress(task_id)))
    }
    fn delete_job(&self, py: Python<'_>, task_id: &str) -> PyResult<String> {
        json(py.detach(|| self.inner.delete_job(task_id)))
    }
    fn download_job_log(&self, py: Python<'_>, task_id: &str, dest: &str) -> PyResult<String> {
        py.detach(|| self.inner.download_job_log(task_id, Path::new(dest)))
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(py_error)
    }

    fn upload_job_package(&self, py: Python<'_>, package: &str) -> PyResult<String> {
        json(py.detach(|| self.inner.upload_job_package(Path::new(package))))
    }
    fn submit_job(&self, py: Python<'_>, options_json: &str) -> PyResult<String> {
        let options = options(options_json)?;
        json(py.detach(|| self.inner.submit_job(&options)))
    }
    fn get_job(&self, py: Python<'_>, task_id: &str) -> PyResult<String> {
        json(py.detach(|| self.inner.get_job(task_id)))
    }
    #[pyo3(signature = (task_id, timeout=3600.0, poll_interval=5.0))]
    fn wait_job(
        &self,
        py: Python<'_>,
        task_id: &str,
        timeout: f64,
        poll_interval: f64,
    ) -> PyResult<String> {
        json(py.detach(|| self.inner.wait_job(task_id, timeout, poll_interval)))
    }
    fn download_job_result(&self, py: Python<'_>, task_id: &str, dest: &str) -> PyResult<String> {
        py.detach(|| self.inner.download_job_result(task_id, Path::new(dest)))
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(py_error)
    }

    fn upload_package(&self, py: Python<'_>, package: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| self.inner.upload_package(Path::new(package))))
    }
    fn save(&self, py: Python<'_>, options_json: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        let options = options(options_json)?;
        json(py.detach(|| self.inner.save(&options)))
    }
    fn submit(&self, py: Python<'_>, options_json: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        let options = options(options_json)?;
        json(py.detach(|| self.inner.submit(&options)))
    }
    #[pyo3(signature = (task_id, preference="balanced", quantum_hint=None, hpc_hint=None))]
    fn recommend(
        &self,
        py: Python<'_>,
        task_id: &str,
        preference: &str,
        quantum_hint: Option<&str>,
        hpc_hint: Option<&str>,
    ) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| {
            self.inner
                .recommend(task_id, preference, quantum_hint, hpc_hint)
        }))
    }
    fn confirm(&self, py: Python<'_>, task_id: &str, plan_id: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| self.inner.confirm(task_id, plan_id)))
    }
    fn list_tasks(&self, py: Python<'_>, query_json: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        let query: Vec<(String, String)> =
            serde_json::from_str(query_json).map_err(|e| py_error(e.into()))?;
        json(py.detach(|| self.inner.list_tasks(&query)))
    }
    fn get(&self, py: Python<'_>, task_id: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| self.inner.get(task_id)))
    }
    fn events(&self, py: Python<'_>, task_id: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| self.inner.events(task_id)))
    }
    fn subtasks(&self, py: Python<'_>, task_id: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| self.inner.subtasks(task_id)))
    }
    fn delete(&self, py: Python<'_>, task_id: &str) -> PyResult<()> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        py.detach(|| self.inner.delete(task_id)).map_err(py_error)
    }
    #[pyo3(signature = (task_id, timeout=3600.0, poll_interval=5.0))]
    fn wait(
        &self,
        py: Python<'_>,
        task_id: &str,
        timeout: f64,
        poll_interval: f64,
    ) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        json(py.detach(|| self.inner.wait(task_id, timeout, poll_interval)))
    }
    fn download_results(&self, py: Python<'_>, task_id: &str, dest: &str) -> PyResult<String> {
        py.import("warnings")?.call_method1(
            "warn",
            (
                "Legacy /sdk/fusion API is not deployed by the platform; use the v2 job API",
                py.get_type::<pyo3::exceptions::PyDeprecationWarning>(),
                2,
            ),
        )?;
        py.detach(|| self.inner.download_results(task_id, Path::new(dest)))
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(py_error)
    }
}

#[pyclass(name = "FusionSchedule", module = "cqlib_tianyan")]
pub struct PyFusionSchedule {
    pub(crate) inner: cqlib_tianyan::fusion::FusionSchedule,
}

#[pymethods]
impl PyFusionSchedule {
    fn candidates<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let text = serde_json::to_string(&self.inner.candidates).map_err(|e| py_error(e.into()))?;
        py.import("json")?.call_method1("loads", (text,))
    }

    fn select(&self, rank: i64) -> PyResult<PyFusionDecision> {
        self.inner
            .select(rank)
            .map(|inner| PyFusionDecision { inner })
            .map_err(py_error)
    }

    fn manual(&self, qpu: &str, cpu: &str) -> PyResult<PyFusionDecision> {
        self.inner
            .manual(qpu, cpu)
            .map(|inner| PyFusionDecision { inner })
            .map_err(py_error)
    }
}

#[pyclass(name = "FusionDecision", module = "cqlib_tianyan")]
pub struct PyFusionDecision {
    pub(crate) inner: cqlib_tianyan::fusion::FusionDecision,
}

#[pyclass(name = "FusionJob", module = "cqlib_tianyan")]
pub struct PyFusionJob {
    pub(crate) client: FusionClient,
    pub(crate) task_id: String,
    pub(crate) snapshot: String,
}

#[pymethods]
impl PyFusionJob {
    fn task_id(&self) -> &str {
        &self.task_id
    }

    fn status(&self) -> &str {
        &self.snapshot
    }

    #[pyo3(signature = (timeout=3600.0, poll_interval=5.0))]
    fn wait(&self, py: Python<'_>, timeout: f64, poll_interval: f64) -> PyResult<String> {
        json(py.detach(|| self.client.wait_job(&self.task_id, timeout, poll_interval)))
    }

    fn download(&self, py: Python<'_>, dest: &str) -> PyResult<String> {
        py.detach(|| {
            self.client
                .download_job_result(&self.task_id, Path::new(dest))
        })
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(py_error)
    }
}
