//! Validate task archives before any upload or authentication request.
use super::FusionError;
use std::{fs::File, io::Read, path::Path};

const MAX_BYTES: u64 = 100 * 1024 * 1024;

pub(super) fn validate(package: &Path) -> Result<(), FusionError> {
    let file = File::open(package)
        .map_err(|_| FusionError::invalid("package must be a readable ZIP file"))?;
    if file.metadata()?.len() > MAX_BYTES {
        return Err(FusionError::invalid("ZIP exceeds the 100 MB upload limit"));
    }
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| FusionError::invalid("package is not a valid ZIP archive"))?;
    let mut main_count = 0;
    let mut qcis_count = 0;
    let mut env_count = 0;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| FusionError::invalid("cannot read ZIP entry"))?;
        if entry.is_dir() {
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > MAX_BYTES {
            return Err(FusionError::invalid(
                "uncompressed ZIP exceeds the 100 MB limit",
            ));
        }
        if entry.name() == "main.py" {
            main_count += 1;
        }
        if entry.name().ends_with(".qcis") {
            qcis_count += 1;
        }
        if entry.name() == "tianyan.env" {
            env_count += 1;
            // Read only the contract's task configuration, never local credentials.
            let mut content = String::new();
            entry
                .by_ref()
                .take(MAX_BYTES + 1)
                .read_to_string(&mut content)
                .map_err(|_| FusionError::invalid("tianyan.env must be valid UTF-8"))?;
            if content.len() as u64 > MAX_BYTES {
                return Err(FusionError::invalid("tianyan.env exceeds the 100 MB limit"));
            }
            validate_shots(&content)?;
        }
    }
    if main_count != 1 {
        return Err(FusionError::invalid(
            "ZIP root must contain exactly one main.py",
        ));
    }
    if qcis_count != 1 {
        return Err(FusionError::invalid(format!(
            "ZIP must contain exactly one .qcis file (found {qcis_count})"
        )));
    }
    if env_count > 1 {
        return Err(FusionError::invalid(
            "ZIP contains duplicate tianyan.env entries",
        ));
    }
    Ok(())
}

fn validate_shots(content: &str) -> Result<(), FusionError> {
    let values: Vec<_> = content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#') || line.is_empty() {
                return None;
            }
            let (key, value) = line.split_once('=').unwrap_or((line, ""));
            (key.trim() == "TIANYAN_APP_SHOTS").then_some(value.trim())
        })
        .collect();
    if values.len() != 1 {
        return Err(FusionError::invalid(
            "tianyan.env must define TIANYAN_APP_SHOTS exactly once",
        ));
    }
    let value = values[0];
    if value.is_empty()
        || !value.bytes().all(|c| c.is_ascii_digit())
        || value
            .parse::<i32>()
            .ok()
            .filter(|shots| *shots > 0)
            .is_none()
    {
        return Err(FusionError::invalid(
            "TIANYAN_APP_SHOTS must be a positive 32-bit integer",
        ));
    }
    Ok(())
}
