//! Revision tracking for the local provider configuration and its referenced files.
use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use jsonc_parser::{JsonValue, ParseOptions, parse_to_value};

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

pub fn revision(profile: &Path) -> Result<u64> {
    let config = crate::provider_setup::configuration_file(profile);
    let text = fs::read_to_string(&config).context("The provider configuration is unavailable")?;
    let root = parse_to_value(&text, &ParseOptions::default())
        .ok()
        .flatten()
        .filter(|value| matches!(value, JsonValue::Object(_)))
        .context(
            "Provider configuration contains invalid JSONC. Correct opencode.jsonc to reload it.",
        )?;
    let mut paths = BTreeSet::new();
    paths.insert(config.clone());
    let configurations = crate::windows_package::configuration_dir(profile);
    paths.insert(configurations.join("agent-instructions.md"));
    for path in [
        crate::windows_package::runtime_dir(profile).join("providers/data/opencode/auth.json"),
        configurations.join("agent-preferences.json"),
    ] {
        if let Ok(bytes) = fs::read(&path)
            && serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .is_none_or(|value| !value.is_object())
        {
            bail!(
                "A provider credential or agent preferences file contains invalid JSON. Correct it to reload the configuration."
            );
        }
        paths.insert(path);
    }
    crate::provider_setup::runtime_overrides(profile)?;
    let directory = config
        .parent()
        .context("The provider directory is unavailable")?;
    collect_references(root, directory, &mut paths)?;
    let keys = directory.join("keys");
    if keys.is_dir() {
        for entry in fs::read_dir(keys)? {
            let path = entry?.path();
            if path.is_file() {
                paths.insert(path);
            }
        }
    }
    let mut hash = DefaultHasher::new();
    for path in paths {
        path.hash(&mut hash);
        match fs::read(path) {
            Ok(bytes) => Some(bytes).hash(&mut hash),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                None::<Vec<u8>>.hash(&mut hash)
            }
            Err(error) => {
                return Err(error).context("A provider configuration file could not be read");
            }
        }
    }
    Ok(hash.finish())
}

fn collect_references(
    value: JsonValue<'_>,
    directory: &Path,
    paths: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    match value {
        JsonValue::Object(object) => {
            if let Some(JsonValue::Array(instructions)) = object.get("instructions") {
                for value in instructions.iter() {
                    if let JsonValue::String(path) = value {
                        let path = resolve_path(path, directory);
                        if path.is_file() {
                            paths.insert(path);
                        }
                    }
                }
            }
            for (_, value) in object {
                collect_references(value, directory, paths)?;
            }
        }
        JsonValue::Array(array) => {
            for value in array {
                collect_references(value, directory, paths)?;
            }
        }
        JsonValue::String(value) => {
            if let Some(path) = value
                .strip_prefix("{file:")
                .and_then(|value| value.strip_suffix('}'))
            {
                let path = resolve_path(path, directory);
                if !path.is_file() {
                    bail!(
                        "A referenced provider file is missing. Correct the configuration to reload it."
                    );
                }
                paths.insert(path);
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_path(path: &str, directory: &Path) -> PathBuf {
    if let Some(relative) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
        && let Some(base) = directories::BaseDirs::new()
    {
        return base.home_dir().join(relative);
    }
    directory.join(path)
}

#[cfg(test)]
#[path = "provider_configuration_tests.rs"]
mod tests;
