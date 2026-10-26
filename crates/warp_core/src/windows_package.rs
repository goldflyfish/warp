use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context as _, Result};
use directories::BaseDirs;

use crate::channel::ChannelState;

pub const PORTABLE_MARKER: &str = "mywarp-portable";
pub const INSTALLED_MARKER: &str = "mywarp-installed";

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
        Some(Package::Portable(directory.join("profile")))
    } else if directory.join(INSTALLED_MARKER).is_file() {
        local_data.map(|data| Package::Installed(data.join("MyWarp")))
    } else {
        None
    }
}

pub fn profile_dir() -> Option<&'static Path> {
    PACKAGE.as_ref().map(Package::profile_dir)
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
    match OpenOptions::new().write(true).create_new(true).open(file) {
        Ok(mut file) => file
            .write_all(b"{\"$schema\":\"https://opencode.ai/config.json\"}\n")
            .context("Could not initialize the MyWarp provider configuration"),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error).context("Could not open the MyWarp provider configuration"),
    }
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
