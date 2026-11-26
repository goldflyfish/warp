//! Windows protection for MyWarp's saved provider API keys.
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::CstRootNode;
use parking_lot::Mutex;
use serde_json::{Value, json};
use uuid::Uuid;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::w;

use crate::windows_package;

const PREFIX: &str = "MYWARP-DPAPI-V1\n";
const ENTROPY: &[u8] = b"MyWarp/provider-api-keys/v1";
static MIGRATION_LOCK: Mutex<()> = Mutex::new(());

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().context("The API key is too large")?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> {
    let input = blob(bytes)?;
    let entropy = blob(ENTROPY)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        let result = if protect {
            CryptProtectData(
                &input,
                w!("MyWarp provider API key"),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        result.context("Windows could not protect or unlock the API key. Save a replacement key in AI providers using this Windows account.")?;
        let buffer = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = buffer.to_vec();
        for byte in buffer {
            std::ptr::write_volatile(byte, 0);
        }
        LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(result)
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path
        .parent()
        .context("The provider folder is unavailable")?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .context("Could not save the protected provider configuration")?;
    Ok(())
}

fn protect_file(path: &Path, key: &str) -> Result<()> {
    let encrypted = crypt(key.as_bytes(), true)?;
    atomic_write(
        path,
        format!("{PREFIX}{}", STANDARD.encode(encrypted)).as_bytes(),
    )
}

pub(crate) fn save_key(profile: &Path, key: &str) -> Result<PathBuf> {
    let path = windows_package::configuration_dir(profile)
        .join("providers/keys")
        .join(format!("mywarp-{}.key", Uuid::new_v4()));
    protect_file(&path, key)?;
    Ok(path)
}

pub(crate) fn read_key(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path).context("The saved API key file could not be read")?;
    let encoded = text
        .strip_prefix(PREFIX)
        .context("The API key file is not protected by MyWarp")?;
    let encrypted = STANDARD
        .decode(encoded)
        .ok()
        .context("The protected API key file is damaged")?;
    String::from_utf8(crypt(&encrypted, false)?)
        .ok()
        .context("The protected API key is invalid")
}

fn key_path(reference: &str, directory: &Path) -> Option<PathBuf> {
    let path = reference.strip_prefix("{file:")?.strip_suffix('}')?;
    if let Some(relative) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        return directories::BaseDirs::new().map(|base| base.home_dir().join(relative));
    }
    Some(directory.join(path))
}

fn reference(path: &Path) -> String {
    format!(
        "{{file:./keys/{}}}",
        path.file_name().unwrap().to_string_lossy()
    )
}

fn owned_key(path: &Path, directory: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| !metadata.file_type().is_symlink())
        && path
            .canonicalize()
            .ok()
            .zip(directory.canonicalize().ok())
            .is_some_and(|(path, directory)| path.starts_with(directory))
}

/// Migrate plaintext API keys before starting or reloading a provider runtime.
pub fn protect_profile(profile: &Path) -> Result<()> {
    let _guard = MIGRATION_LOCK.lock();
    let path = windows_package::provider_configuration_file(profile);
    let directory = path
        .parent()
        .context("The provider folder is unavailable")?;
    let original = fs::read_to_string(&path)?;
    let config = jsonc_parser::parse_to_serde_value::<Value>(&original, &ParseOptions::default())
        .ok()
        .filter(Value::is_object)
        .context("The provider configuration contains invalid JSONC")?;
    let mut updates = Vec::new();
    if let Some(providers) = config["provider"].as_object() {
        for (id, provider) in providers {
            let Some(key) = provider["options"]["apiKey"]
                .as_str()
                .filter(|key| !key.trim().is_empty())
            else {
                continue;
            };
            if key.starts_with("{env:") && key.ends_with('}') {
                continue;
            }
            if let Some(key_file) = key_path(key, directory) {
                let text = fs::read_to_string(&key_file)
                    .context("A referenced API key file could not be read")?;
                if text.starts_with(PREFIX) {
                    read_key(&key_file)?;
                    continue;
                }
                if text.starts_with("MYWARP-DPAPI-") {
                    bail!(
                        "The protected API key format is unsupported. Save a replacement key in AI providers."
                    );
                }
                if owned_key(&key_file, directory) {
                    protect_file(&key_file, text.trim())?;
                    continue;
                }
                let saved = save_key(profile, text.trim())?;
                updates.push((id.clone(), reference(&saved)));
            } else {
                let saved = save_key(profile, key.trim())?;
                updates.push((id.clone(), reference(&saved)));
            }
        }
    }

    let auth_path = windows_package::runtime_dir(profile).join("providers/data/opencode/auth.json");
    let auth_bytes = match fs::read(&auth_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).context("The provider auth file could not be read"),
    };
    let mut auth: Value = match &auth_bytes {
        Some(bytes) => serde_json::from_slice(bytes)
            .ok()
            .filter(Value::is_object)
            .context("The provider auth file contains invalid JSON")?,
        None => json!({}),
    };
    let api_ids: Vec<_> = auth
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, value)| value["type"] == "api")
        .map(|(id, _)| id.clone())
        .collect();
    for id in &api_ids {
        let key = auth[id]["key"]
            .as_str()
            .context("A saved provider API key is invalid")?;
        if config["provider"][id]["options"]["apiKey"]
            .as_str()
            .is_none_or(|key| key.trim().is_empty())
        {
            let saved = save_key(profile, key)?;
            updates.push((id.clone(), reference(&saved)));
        }
        auth.as_object_mut().unwrap().remove(id);
    }
    if !updates.is_empty() {
        let root = CstRootNode::parse(&original, &ParseOptions::default())?;
        let providers = root
            .object_value()
            .context("The provider configuration must be an object")?
            .object_value_or_set("provider");
        for (id, key) in updates {
            let options = providers
                .object_value_or_set(&id)
                .object_value_or_set("options");
            let value = jsonc_parser::cst::CstInputValue::String(key);
            if let Some(property) = options.get("apiKey") {
                property.set_value(value);
            } else {
                options.append("apiKey", value);
            }
        }
        if fs::read_to_string(&path)? != original {
            bail!(
                "The provider configuration changed during API key protection. Retry the operation."
            );
        }
        atomic_write(&path, root.to_string().as_bytes())?;
    }
    if !api_ids.is_empty() {
        if fs::read(&auth_path).ok() != auth_bytes {
            bail!(
                "The provider credentials changed during API key protection. Retry the operation."
            );
        }
        atomic_write(&auth_path, &serde_json::to_vec_pretty(&auth)?)?;
    }
    Ok(())
}

/// Resolve protected keys only for the child runtime's in-memory configuration.
pub fn runtime_overrides(profile: &Path) -> Result<Value> {
    protect_profile(profile)?;
    let path = windows_package::provider_configuration_file(profile);
    let config = jsonc_parser::parse_to_serde_value::<Value>(
        &fs::read_to_string(&path)?,
        &ParseOptions::default(),
    )?;
    let mut overrides = json!({});
    if let Some(providers) = config["provider"].as_object() {
        for (id, provider) in providers {
            if let Some(path) = provider["options"]["apiKey"]
                .as_str()
                .and_then(|key| key_path(key, path.parent().unwrap()))
            {
                overrides["provider"][id]["options"]["apiKey"] = read_key(&path)?.into();
            }
        }
    }
    Ok(overrides)
}

#[cfg(test)]
#[path = "provider_credentials_tests.rs"]
mod tests;
