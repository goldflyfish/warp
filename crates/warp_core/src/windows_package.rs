use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context as _, Result};
use directories::BaseDirs;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::CstRootNode;

use crate::channel::ChannelState;

pub const PORTABLE_MARKER: &str = "mywarp-portable";
pub const INSTALLED_MARKER: &str = "mywarp-installed";
pub const CONFIGURATIONS_DIR: &str = "Configurations";
pub const PERSONAL_INSTRUCTIONS_REFERENCE: &str =
    "{env:MYWARP_CONFIGURATIONS_DIR}/agent-instructions.md";
pub const DEFAULT_SETTINGS: &str = "# MyWarp settings: appearance, shortcuts, and agent profiles.\n[agents.execution_profiles.default]\nname = \"Default\"\n";

const MIGRATION_MARKER: &str = ".configuration-layout-v2";

enum Package {
    Portable(PathBuf),
    Installed(PathBuf),
}

impl Package {
    fn profile_dir(&self) -> &Path {
        match self {
            Self::Portable(path) | Self::Installed(path) => path,
        }
    }

    fn instance_namespace(&self) -> String {
        match self {
            Self::Installed(_) => "MyWarpInstalled".to_owned(),
            Self::Portable(path) => {
                let hash = path
                    .to_string_lossy()
                    .to_lowercase()
                    .bytes()
                    .fold(0xcbf29ce484222325_u64, |hash, byte| {
                        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
                    });
                format!("MyWarpPortable{hash:016x}")
            }
        }
    }
}

static PACKAGE: LazyLock<Option<Package>> = LazyLock::new(|| {
    let executable = std::env::current_exe().ok()?;
    let executable = std::fs::canonicalize(&executable).unwrap_or(executable);
    let local_data = BaseDirs::new().map(|dirs| dirs.data_local_dir().to_owned());
    detect_package(&executable, local_data.as_deref())
});

fn detect_package(executable: &Path, local_data: Option<&Path>) -> Option<Package> {
    let directory = executable.parent()?;
    if directory.join(PORTABLE_MARKER).is_file() {
        Some(Package::Portable(directory.to_owned()))
    } else if directory.join(INSTALLED_MARKER).is_file() {
        local_data.map(|data| Package::Installed(data.join("MyWarp")))
    } else {
        None
    }
}

pub fn profile_dir() -> Option<&'static Path> {
    PACKAGE.as_ref().map(Package::profile_dir)
}

pub fn configuration_dir(profile: &Path) -> PathBuf {
    profile.join(CONFIGURATIONS_DIR)
}

pub fn runtime_dir(profile: &Path) -> PathBuf {
    profile.join("Runtime")
}

pub fn provider_configuration_file(profile: &Path) -> PathBuf {
    configuration_dir(profile).join("providers/opencode.jsonc")
}

pub fn prepare_profile() -> Result<()> {
    let Some(package) = PACKAGE.as_ref() else {
        return Ok(());
    };
    let profile = package.profile_dir();
    prepare_profile_at(profile)?;
    prepare_opencode_profile(profile)
}

fn prepare_profile_at(profile: &Path) -> Result<()> {
    let runtime = runtime_dir(profile);
    fs::create_dir_all(&runtime)?;
    if !runtime.join(MIGRATION_MARKER).exists() {
        migrate_profile(&profile.join("profile"), profile)?;
        migrate_profile(&configuration_dir(profile), profile)?;
        migrate_profile(profile, profile)?;
        fs::write(runtime.join(MIGRATION_MARKER), "")?;
    }
    for name in ["config", "state", "data", "cache"] {
        fs::create_dir_all(runtime.join(name))?;
    }
    fs::create_dir_all(configuration_dir(profile).join("themes"))?;
    let settings = configuration_dir(profile).join("settings.toml");
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(settings)
    {
        Ok(mut file) => file.write_all(DEFAULT_SETTINGS.as_bytes())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn migrate_profile(legacy: &Path, profile: &Path) -> Result<()> {
    if !legacy.is_dir() {
        return Ok(());
    }
    let configurations = configuration_dir(profile);
    let runtime = runtime_dir(profile);
    for name in ["settings.toml", ".mcp.json", "skills"] {
        copy_configuration(
            &legacy.join("config").join(name),
            &configurations.join(name),
        )?;
    }
    for name in [
        "themes",
        "workflows",
        "launch_configurations",
        "tab_configs",
        "custom_model_routers",
    ] {
        copy_configuration(&legacy.join("data").join(name), &configurations.join(name))?;
    }
    let provider = legacy.join("providers/config/opencode");
    for name in ["opencode.jsonc", "keys", "agents", "commands", "plugins"] {
        copy_configuration(
            &provider.join(name),
            &configurations.join("providers").join(name),
        )?;
    }
    for name in ["agent-preferences.json", "agent-instructions.md"] {
        copy_configuration(&legacy.join(name), &configurations.join(name))?;
    }
    move_runtime(
        &legacy.join(".profile-migration-in-progress"),
        &runtime.join("legacy-migration-marker"),
    )?;
    for name in [
        "state",
        "cache",
        "data",
        "logs",
        "provider-check",
        "config",
        "providers/data",
        "providers/cache",
        "providers/state",
        "providers/config",
    ] {
        let destination = if name == "providers/config" {
            runtime.join("legacy-provider-config")
        } else {
            runtime.join(name)
        };
        move_runtime(&legacy.join(name), &destination)?;
    }
    if legacy == profile.join("profile") && legacy.is_dir() {
        move_runtime(legacy, &runtime.join("legacy-profile"))?;
    }
    Ok(())
}

fn copy_configuration(source: &Path, destination: &Path) -> Result<()> {
    if source == destination || !source.exists() {
        return Ok(());
    }
    if source.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_configuration(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if !destination.exists() || is_default_configuration(destination) {
        fs::create_dir_all(
            destination
                .parent()
                .context("Configuration has no parent")?,
        )?;
        fs::copy(source, destination)?;
    }
    Ok(())
}

fn is_default_configuration(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    if path.file_name().is_some_and(|name| name == "settings.toml") {
        return text == DEFAULT_SETTINGS;
    }
    path.file_name()
        .is_some_and(|name| name == "opencode.jsonc")
        && serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .is_some_and(|object| object.keys().all(|key| key == "$schema"))
}

fn move_runtime(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() || source == destination {
        return Ok(());
    }
    fs::create_dir_all(destination.parent().context("Runtime data has no parent")?)?;
    let mut target = destination.to_owned();
    let mut suffix = 0;
    while target.exists() {
        suffix += 1;
        target = destination.with_file_name(format!(
            "{}.legacy-{suffix}",
            destination
                .file_name()
                .context("Runtime data has no name")?
                .to_string_lossy()
        ));
    }
    fs::rename(source, target)
        .context("Could not migrate runtime data. Close MyWarp and OpenCode before retrying.")?;
    Ok(())
}

pub fn instance_namespace() -> String {
    PACKAGE.as_ref().map_or_else(
        || format!("Warp{:?}", ChannelState::channel()),
        Package::instance_namespace,
    )
}

pub fn opencode_environment(profile: &Path) -> Vec<(&'static str, PathBuf)> {
    let root = runtime_dir(profile).join("providers");
    let config = root.join("config").join("opencode");
    vec![
        ("XDG_CONFIG_HOME", root.join("config")),
        ("XDG_DATA_HOME", root.join("data")),
        ("XDG_CACHE_HOME", root.join("cache")),
        ("XDG_STATE_HOME", root.join("state")),
        ("OPENCODE_CONFIG", provider_configuration_file(profile)),
        ("OPENCODE_CONFIG_DIR", config),
        (
            "MYWARP_CONFIGURATIONS_DIR",
            jsonc_environment_path(&configuration_dir(profile)),
        ),
    ]
}

fn jsonc_environment_path(path: &Path) -> PathBuf {
    // OpenCode substitutes environment values before parsing JSONC.
    let path = path.to_string_lossy();
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!("//{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    };
    PathBuf::from(path.replace('\\', "/"))
}

pub fn prepare_opencode_profile(profile: &Path) -> Result<()> {
    prepare_profile_at(profile)?;
    let root = runtime_dir(profile).join("providers");
    let config = configuration_dir(profile).join("providers");
    for directory in [
        &config,
        &root.join("data"),
        &root.join("cache"),
        &root.join("state"),
        &root.join("config/opencode"),
    ] {
        fs::create_dir_all(directory).context("Could not create the MyWarp provider profile")?;
    }
    let file = config.join("opencode.jsonc");
    match OpenOptions::new().write(true).create_new(true).open(&file) {
        Ok(mut file) => file
            .write_all(b"{\"$schema\":\"https://opencode.ai/config.json\"}\n")
            .context("Could not initialize the MyWarp provider configuration"),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error).context("Could not open the MyWarp provider configuration"),
    }?;
    rebase_personal_instructions(profile, &file)
}

fn rebase_personal_instructions(profile: &Path, config: &Path) -> Result<()> {
    let instructions = configuration_dir(profile).join("agent-instructions.md");
    if !instructions.is_file() {
        return Ok(());
    }
    let Ok(original) = fs::read_to_string(config) else {
        return Ok(());
    };
    let Ok(root) = CstRootNode::parse(&original, &ParseOptions::default()) else {
        return Ok(());
    };
    let Some(paths) = root
        .object_value()
        .and_then(|object| object.array_value("instructions"))
    else {
        return Ok(());
    };
    let mut changed = false;
    for value in paths.elements() {
        let Some(value) = value.as_string_lit() else {
            continue;
        };
        let path = value.decoded_value()?;
        let path = Path::new(&path);
        let managed_profile = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                ["profile", "MyWarp", CONFIGURATIONS_DIR]
                    .iter()
                    .any(|known| name.eq_ignore_ascii_case(known))
            });
        if path.is_absolute()
            && path
                .file_name()
                .is_some_and(|name| name == "agent-instructions.md")
            && managed_profile
        {
            value.set_raw_value(serde_json::to_string(PERSONAL_INSTRUCTIONS_REFERENCE)?);
            changed = true;
        }
    }
    if changed {
        let mut temporary = tempfile::NamedTempFile::new_in(
            config
                .parent()
                .context("The provider configuration has no parent")?,
        )?;
        temporary.write_all(root.to_string().as_bytes())?;
        temporary
            .persist(config)
            .context("Could not update the personal instructions path")?;
    }
    Ok(())
}

pub fn import_existing_opencode_profile(profile: &Path) -> Result<()> {
    let Some(base) = BaseDirs::new() else {
        return Ok(());
    };
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.home_dir().join(".config"));
    let config = std::env::var_os("OPENCODE_CONFIG")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            ["opencode.jsonc", "opencode.json"]
                .into_iter()
                .map(|name| config_home.join("opencode").join(name))
                .find(|path| path.is_file())
        });
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.home_dir().join(".local/share"));
    import_opencode_configuration(
        profile,
        config.as_deref(),
        &data_home.join("opencode/auth.json"),
    )
}

fn import_opencode_configuration(profile: &Path, source: Option<&Path>, auth: &Path) -> Result<()> {
    let config_dir = configuration_dir(profile).join("providers");
    let destination = config_dir.join("opencode.jsonc");
    let auth_destination = runtime_dir(profile).join("providers/data/opencode/auth.json");
    if auth.is_file() && !auth_destination.exists() {
        fs::create_dir_all(auth_destination.parent().expect("auth parent"))?;
        fs::copy(auth, auth_destination).context("Could not import existing provider sign-ins")?;
    }
    let current: serde_json::Value = match serde_json::from_slice(&fs::read(&destination)?) {
        Ok(config) => config,
        Err(_) => return Ok(()),
    };
    if current
        .as_object()
        .is_none_or(|object| object.keys().any(|key| key != "$schema"))
    {
        return Ok(());
    }
    if let Some(source) = source.filter(|source| *source != destination) {
        let original =
            fs::read_to_string(source).context("Could not read existing provider configuration")?;
        let references =
            regex::Regex::new(r"\{file:([^}]+)\}").expect("static file-reference pattern");
        let mut migrated = String::new();
        let mut offset = 0;
        for (index, capture) in references.captures_iter(&original).enumerate() {
            let whole = capture.get(0).expect("whole match");
            let path = PathBuf::from(&capture[1]);
            let path = if path.is_absolute() {
                path
            } else {
                source.parent().unwrap_or(Path::new(".")).join(path)
            };
            let relative = format!("keys/imported-{index}.key");
            fs::create_dir_all(config_dir.join("keys"))?;
            let content = fs::read(&path)
                .context("Could not read a file used by the existing provider configuration")?;
            fs::write(config_dir.join(&relative), content)?;
            migrated.push_str(&original[offset..whole.start()]);
            migrated.push_str(&format!("{{file:./{relative}}}"));
            offset = whole.end();
        }
        migrated.push_str(&original[offset..]);
        fs::write(&destination, migrated)
            .context("Could not import provider configuration into the MyWarp profile")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "windows_package_tests.rs"]
mod tests;
