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

#[cfg(test)]
#[path = "windows_package_tests.rs"]
mod tests;
