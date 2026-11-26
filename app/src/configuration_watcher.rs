//! Runtime notification of valid edits to the application's provider profile.
use std::time::Duration;

use warpui::r#async::Timer;
use warpui::{Entity, ModelContext, SingletonEntity};

#[derive(Clone)]
pub(crate) enum ConfigurationEvent {
    Changed,
    Invalid(String),
    Checked,
}

pub(crate) struct ConfigurationWatcher {
    revision: Option<u64>,
    error: Option<String>,
}

impl Entity for ConfigurationWatcher {
    type Event = ConfigurationEvent;
}
impl SingletonEntity for ConfigurationWatcher {}

impl ConfigurationWatcher {
    pub(crate) fn new(ctx: &mut ModelContext<Self>) -> Self {
        Self::schedule(Duration::ZERO, ctx);
        Self {
            revision: None,
            error: None,
        }
    }

    fn schedule(delay: Duration, ctx: &mut ModelContext<Self>) {
        ctx.spawn(
            async move {
                Timer::after(delay).await;
                warp_core::provider_setup::offload(|| {
                    warp_core::provider_configuration::revision(
                        &warp_core::provider_setup::profile_dir()?,
                    )
                })
                .await
            },
            |me, result, ctx| {
                match result {
                    Ok(revision) => {
                        let recovered = me.error.take().is_some();
                        let changed = me.revision.is_some_and(|previous| previous != revision);
                        me.revision = Some(revision);
                        if changed || recovered {
                            ctx.emit(ConfigurationEvent::Changed);
                        }
                        ctx.emit(ConfigurationEvent::Checked);
                    }
                    Err(error) => {
                        let error = error.to_string();
                        if me.error.as_ref() != Some(&error) {
                            me.error = Some(error.clone());
                            ctx.emit(ConfigurationEvent::Invalid(error));
                        }
                    }
                }
                Self::schedule(warp_core::provider_configuration::REFRESH_INTERVAL, ctx);
            },
        );
    }
}
