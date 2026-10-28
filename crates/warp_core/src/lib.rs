pub mod app_id;
pub mod assertions;
pub mod r#async;
#[cfg(windows)]
pub mod background_provider;
pub mod channel;
pub mod cli_agent_protocol;
pub mod command;
pub mod context_flag;
pub mod execution_mode;
pub mod features;
pub mod interval_timer;
#[cfg(windows)]
pub mod local_speech;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod mywarp;
pub mod operating_system_info;
pub mod paths;
pub mod platform;
#[cfg(windows)]
pub mod provider_configuration;
#[cfg(windows)]
pub mod provider_setup;
pub mod safe_log;
pub mod semantic_selection;
#[cfg(windows)]
pub mod windows_package;
pub use settings;
// Re-export settings macros for backward compatibility
pub use settings::{
    define_setting, define_settings_group, implement_setting_for_enum, maybe_define_setting,
};
pub mod session_id;
pub mod sync_queue;
pub mod telemetry;
pub mod ui;
pub mod user_preferences;

#[doc(hidden)]
pub use anyhow as __anyhow;
pub use app_id::AppId;
pub use session_id::SessionId;
#[doc(hidden)]
pub use warp_errors as __warp_errors;
pub use warp_util::host_id::HostId;
// Re-export warpui_core so that it can be referenced safely from the
// telemetry macros.
pub use warpui_core;
