//! Service policy for the independent MyWarp application.

use url::Url;

use crate::channel::{Channel, ChannelState};

pub fn enabled() -> bool {
    ChannelState::channel() == Channel::Oss
}

pub fn is_warp_service(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().trim_end_matches('.');
    [
        "warp.dev",
        "warpdotdev.com",
        "warpdrive.dev",
        "warp-terminal.com",
    ]
    .iter()
    .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        || matches!(
            host,
            "identitytoolkit.googleapis.com" | "securetoken.googleapis.com"
        )
        || (host == "127.0.0.1" && url.port() == Some(0))
}

pub fn blocks_url(url: &Url) -> bool {
    enabled() && is_warp_service(url)
}

#[cfg(test)]
#[path = "mywarp_tests.rs"]
mod tests;
