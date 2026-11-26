use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::windows_package;

type Records = BTreeMap<String, BTreeMap<String, String>>;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

fn read(profile: &Path) -> Records {
    fs::read(windows_package::configuration_dir(profile).join("providers/verification.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub(super) fn save(
    profile: &Path,
    id: &str,
    model: &str,
    fingerprint: Option<String>,
) -> Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut records = read(profile);
    if let Some(fingerprint) = fingerprint {
        records
            .entry(id.into())
            .or_default()
            .insert(model.into(), fingerprint);
    } else if let Some(models) = records.get_mut(id) {
        models.remove(model);
        if models.is_empty() {
            records.remove(id);
        }
    }
    let directory = windows_package::configuration_dir(profile).join("providers");
    fs::create_dir_all(&directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    temporary.write_all(&serde_json::to_vec_pretty(&records)?)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(directory.join("verification.json"))
        .context("Could not save provider verification. Test the connection again to retry.")?;
    Ok(())
}

pub(super) fn load(
    profile: &Path,
    config: &Value,
    providers: &Value,
) -> BTreeSet<(String, String)> {
    let mut verified = BTreeSet::new();
    for (id, models) in read(profile) {
        let Some(provider) = providers["all"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|provider| provider["id"] == id)
        else {
            continue;
        };
        if !providers["connected"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|value| value == &id)
        {
            continue;
        }
        for (model, saved) in models {
            if provider["models"].get(&model).is_some()
                && fingerprint(profile, &id, &model, config, provider)
                    .is_ok_and(|current| current == saved)
            {
                verified.insert((id.clone(), model));
            }
        }
    }
    verified
}

pub(super) fn fingerprint(
    profile: &Path,
    id: &str,
    model: &str,
    config: &Value,
    provider: &Value,
) -> Result<String> {
    let directory = windows_package::configuration_dir(profile).join("providers");
    let mut connection = config["provider"][id].clone();
    if let Some(models) = connection.get_mut("models").and_then(Value::as_object_mut) {
        models.retain(|name, _| name == model);
        if models.is_empty() {
            connection.as_object_mut().unwrap().remove("models");
        }
    }
    resolve_references(&mut connection, &directory)?;
    let auth_path = windows_package::runtime_dir(profile).join("providers/data/opencode/auth.json");
    let auth: Value = match fs::read(auth_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("The provider auth file is invalid")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(error) => return Err(error).context("Could not read the provider auth file"),
    };
    let environment: BTreeMap<_, _> = provider["env"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|name| (name, std::env::var(name).ok()))
        .collect();
    let disabled = config["disabled_providers"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|value| value == id);
    let mut input = json!({
        "version": 1, "id": id, "model": model, "connection": connection,
        "auth": auth[id], "environment": environment, "disabled": disabled,
        "api": provider["models"][model]["api"], "npm": provider["npm"],
    });
    input.sort_all_objects();
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&input)?)))
}

fn resolve_references(value: &mut Value, directory: &Path) -> Result<()> {
    match value {
        Value::Object(values) => {
            for value in values.values_mut() {
                resolve_references(value, directory)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                resolve_references(value, directory)?;
            }
        }
        Value::String(text) => {
            if let Some(path) = text
                .strip_prefix("{file:")
                .and_then(|path| path.strip_suffix('}'))
            {
                let path = if let Some(relative) =
                    path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
                {
                    directories::BaseDirs::new()
                        .context("Your home directory is unavailable")?
                        .home_dir()
                        .join(relative)
                } else {
                    directory.join(path)
                };
                *text = fs::read_to_string(path)
                    .context("A referenced provider file could not be read")?;
            } else if let Some(name) = text
                .strip_prefix("{env:")
                .and_then(|name| name.strip_suffix('}'))
            {
                *text = std::env::var(name).unwrap_or_default();
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}

#[cfg(test)]
#[path = "provider_verification_tests.rs"]
mod tests;
