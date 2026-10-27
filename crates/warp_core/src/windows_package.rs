use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context as _, Result, bail};
use directories::BaseDirs;
use jsonc_parser::ParseOptions;
use jsonc_parser::cst::CstRootNode;

use crate::channel::ChannelState;

pub const PORTABLE_MARKER: &str = "mywarp-portable";
pub const INSTALLED_MARKER: &str = "mywarp-installed";
pub const CONFIGURATIONS_DIR: &str = "Configurations";

const MIGRATION_MARKER: &str = ".profile-migration-in-progress";

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
        Some(Package::Portable(directory.join(CONFIGURATIONS_DIR)))
    } else if directory.join(INSTALLED_MARKER).is_file() {
        local_data.map(|data| Package::Installed(data.join("MyWarp").join(CONFIGURATIONS_DIR)))
    } else {
        None
    }
}

pub fn profile_dir() -> Option<&'static Path> {
    PACKAGE.as_ref().map(Package::profile_dir)
}

pub fn prepare_profile() -> Result<()> {
    let Some(package) = PACKAGE.as_ref() else {
        return Ok(());
    };
    let profile = package.profile_dir();
    let legacy = match package {
        Package::Portable(_) => profile.with_file_name("profile"),
        Package::Installed(_) => profile
            .parent()
            .context("The MyWarp profile has no parent")?
            .to_owned(),
    };
    migrate_profile(&legacy, profile)?;
    for name in ["config", "state", "data", "cache"] {
        fs::create_dir_all(profile.join(name))?;
    }
    prepare_opencode_profile(profile)
}

fn migrate_profile(legacy: &Path, profile: &Path) -> Result<()> {
    if !legacy.is_dir() {
        return Ok(());
    }
    let marker = profile.join(MIGRATION_MARKER);
    if profile.is_dir() && !marker.is_file() && fs::read_dir(profile)?.next().is_some() {
        return Ok(());
    }
    fs::create_dir_all(profile).context("Could not create the Configurations folder")?;
    fs::write(&marker, "").context("Could not start migrating the MyWarp profile")?;
    for entry in fs::read_dir(legacy)? {
        let entry = entry?;
        if entry.path() == profile {
            continue;
        }
        let destination = profile.join(entry.file_name());
        if destination.try_exists()? {
            bail!(
                "A file in Configurations conflicts with the unfinished profile migration. Keep the desired file and remove its duplicate from the old profile before retrying."
            );
        }
        fs::rename(entry.path(), destination)
            .context("Could not move the existing MyWarp profile into Configurations. Close MyWarp and OpenCode before retrying.")?;
    }
    if legacy != profile.parent().unwrap_or(profile) {
        fs::remove_dir(legacy).context("Could not remove the empty legacy profile folder")?;
    }
    fs::remove_file(marker).context("Could not finish migrating the MyWarp profile")?;
    Ok(())
}

pub fn instance_namespace() -> String {
    PACKAGE.as_ref().map_or_else(
        || format!("Warp{:?}", ChannelState::channel()),
        Package::instance_namespace,
    )
}

pub fn opencode_environment(profile: &Path) -> Vec<(&'static str, PathBuf)> {
    let root = profile.join("providers");
    let config = root.join("config").join("opencode");
    vec![
        ("XDG_CONFIG_HOME", root.join("config")),
        ("XDG_DATA_HOME", root.join("data")),
        ("XDG_CACHE_HOME", root.join("cache")),
        ("XDG_STATE_HOME", root.join("state")),
        ("OPENCODE_CONFIG", config.join("opencode.jsonc")),
        ("OPENCODE_CONFIG_DIR", config),
    ]
}

pub fn prepare_opencode_profile(profile: &Path) -> Result<()> {
    let root = profile.join("providers");
    let config = root.join("config").join("opencode");
    for directory in [
        &config,
        &root.join("data"),
        &root.join("cache"),
        &root.join("state"),
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
    let instructions = profile.join("agent-instructions.md");
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
            && path != instructions
        {
            value.set_raw_value(serde_json::to_string(&instructions.to_string_lossy())?);
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
    let config_dir = profile.join("providers/config/opencode");
    let destination = config_dir.join("opencode.jsonc");
    let auth_destination = profile.join("providers/data/opencode/auth.json");
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
