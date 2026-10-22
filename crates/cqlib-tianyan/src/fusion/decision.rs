//! Pure selection rules shared by the hybrid job client.
//! Resource codes match qccp-lab `candidateResources`: `code` equals the
//! recommendation `device_id` or `resource_id`. Display names are not used.
use super::FusionError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceChoice {
    pub code: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionCandidate {
    pub rank: i64,
    pub group_id: String,
    pub device_id: String,
    pub resource_id: String,
    pub qpu_code: Option<String>,
    pub cpu_code: Option<String>,
    pub meets_all_constraints: bool,
    pub scores: Value,
    pub selectable: bool,
    pub reject_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionSchedule {
    pub file_id: String,
    pub candidates: Vec<FusionCandidate>,
    pub(crate) choices: ResourceChoices,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct ResourceChoices {
    pub qpu: Vec<ResourceChoice>,
    pub cpu: Vec<ResourceChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionDecision {
    pub file_id: String,
    pub qpu_code: String,
    pub cpu_code: String,
    pub scheduling_mode: String,
    pub scheduling_preference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitSignal {
    Continue,
    Ready,
    Error,
}

pub fn top_candidates(mut ranked: Vec<FusionCandidate>) -> Vec<FusionCandidate> {
    ranked.sort_by_key(|item| item.rank);
    ranked.truncate(3);
    ranked
}

pub fn select_rank(schedule: &FusionSchedule, rank: i64) -> Result<FusionDecision, FusionError> {
    let candidate = schedule
        .candidates
        .iter()
        .find(|item| item.rank == rank)
        .ok_or_else(|| FusionError::invalid("selected rank is not in the top 3 recommendations"))?;
    if !candidate.meets_all_constraints {
        return Err(FusionError::invalid(
            "selected recommendation does not meet all constraints",
        ));
    }
    let (qpu, cpu) = match_resources(
        &candidate.device_id,
        &candidate.resource_id,
        &schedule.choices,
    )?;
    Ok(FusionDecision {
        file_id: schedule.file_id.clone(),
        qpu_code: qpu,
        cpu_code: cpu,
        scheduling_mode: "SMART".into(),
        scheduling_preference: None,
    })
}

pub fn select_manual(
    schedule: &FusionSchedule,
    qpu: &str,
    cpu: &str,
) -> Result<FusionDecision, FusionError> {
    let (qpu_code, cpu_code) = match_resources(qpu, cpu, &schedule.choices)?;
    Ok(FusionDecision {
        file_id: schedule.file_id.clone(),
        qpu_code,
        cpu_code,
        scheduling_mode: "MANUAL".into(),
        scheduling_preference: None,
    })
}

pub fn wait_signal(status: i64) -> Result<WaitSignal, FusionError> {
    match status {
        5 | 6 | 7 => Ok(WaitSignal::Ready),
        1 | 2 | 3 | 4 | 8 => Ok(WaitSignal::Continue),
        9 => Err(FusionError::invalid("fusion task status is unknown")),
        other => Err(FusionError::invalid(format!(
            "unexpected fusion task status {other}"
        ))),
    }
}

pub fn annotate(
    device_id: &str,
    resource_id: &str,
    meets_all_constraints: bool,
    choices: &ResourceChoices,
) -> (Option<String>, Option<String>, bool, Option<String>) {
    match match_resources(device_id, resource_id, choices) {
        Ok((qpu, cpu)) if meets_all_constraints => (Some(qpu), Some(cpu), true, None),
        Ok((qpu, cpu)) => (
            Some(qpu),
            Some(cpu),
            false,
            Some("selected recommendation does not meet all constraints".into()),
        ),
        Err(error) => (None, None, false, Some(error.message)),
    }
}

fn match_resources(
    qpu: &str,
    cpu: &str,
    choices: &ResourceChoices,
) -> Result<(String, String), FusionError> {
    let qpu_code = available_code(&choices.qpu, qpu, "QPU")?;
    let cpu_code = available_code(&choices.cpu, cpu, "CPU")?;
    Ok((qpu_code, cpu_code))
}

fn available_code(
    choices: &[ResourceChoice],
    code: &str,
    kind: &str,
) -> Result<String, FusionError> {
    let found = choices.iter().find(|item| item.code == code);
    match found {
        Some(item) if item.available => Ok(item.code.clone()),
        Some(_) => Err(FusionError::invalid(format!(
            "{kind} resource {code} is unavailable"
        ))),
        None => Err(FusionError::invalid(format!(
            "{kind} resource {code} is not in the resource list"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn choices() -> ResourceChoices {
        ResourceChoices {
            qpu: vec![ResourceChoice {
                code: "qpu-1".into(),
                available: true,
            }],
            cpu: vec![ResourceChoice {
                code: "cpu-1".into(),
                available: true,
            }],
        }
    }

    fn candidate(rank: i64, meets: bool) -> FusionCandidate {
        let (qpu, cpu, selectable, reason) = annotate("qpu-1", "cpu-1", meets, &choices());
        FusionCandidate {
            rank,
            group_id: format!("rg-{rank}"),
            device_id: "qpu-1".into(),
            resource_id: "cpu-1".into(),
            qpu_code: qpu,
            cpu_code: cpu,
            meets_all_constraints: meets,
            scores: json!({"quality": 1}),
            selectable,
            reject_reason: reason,
        }
    }

    #[test]
    fn keeps_only_three_lowest_ranks() {
        let visible = top_candidates(vec![
            candidate(4, true),
            candidate(1, true),
            candidate(3, true),
            candidate(2, true),
        ]);
        assert_eq!(
            visible.iter().map(|item| item.rank).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn select_rejects_constraints_and_unknown_rank() {
        let schedule = FusionSchedule {
            file_id: "file-1".into(),
            candidates: vec![candidate(1, false)],
            choices: choices(),
        };
        assert!(
            select_rank(&schedule, 1)
                .unwrap_err()
                .message
                .contains("constraints")
        );
        assert!(select_rank(&schedule, 2).is_err());
    }

    #[test]
    fn manual_matches_codes_not_names() {
        let schedule = FusionSchedule {
            file_id: "file-1".into(),
            candidates: vec![],
            choices: choices(),
        };
        let decision = select_manual(&schedule, "qpu-1", "cpu-1").unwrap();
        assert_eq!(decision.scheduling_mode, "MANUAL");
        assert!(select_manual(&schedule, "量子资源 A", "cpu-1").is_err());
    }

    #[test]
    fn unavailable_code_fails_before_submit() {
        let mut resources = choices();
        resources.qpu[0].available = false;
        let error = match_resources("qpu-1", "cpu-1", &resources).unwrap_err();
        assert!(error.message.contains("unavailable"));
    }

    #[test]
    fn paused_status_continues_waiting_and_unknown_status_is_error() {
        assert_eq!(wait_signal(6).unwrap(), WaitSignal::Ready);
        assert_eq!(wait_signal(8).unwrap(), WaitSignal::Continue);
        assert_eq!(wait_signal(3).unwrap(), WaitSignal::Continue);
        assert!(wait_signal(9).is_err());
    }
}
