//! Tool-free background generation using the application's configured provider.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use instant::Instant;
use parking_lot::Mutex;

use crate::provider_setup::{self, ProviderServer};

struct CachedServer {
    profile: PathBuf,
    revision: u64,
    server: Arc<ProviderServer>,
}

static SERVER: LazyLock<Mutex<Option<CachedServer>>> = LazyLock::new(|| Mutex::new(None));
static GENERATION: Mutex<()> = Mutex::new(());

struct Cancellation(Arc<AtomicBool>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn server(profile: PathBuf) -> Result<Arc<ProviderServer>> {
    let revision = crate::provider_configuration::revision(&profile)?;
    let mut cached = SERVER.lock();
    if let Some(cached) = cached.as_ref()
        && cached.profile == profile
        && cached.revision == revision
    {
        return Ok(cached.server.clone());
    }
    let server = ProviderServer::start(profile.clone(), provider_setup::runtime_executable())?;
    *cached = Some(CachedServer {
        profile,
        revision,
        server: server.clone(),
    });
    Ok(server)
}

pub async fn complete(feature: &'static str, system: String, prompt: String) -> Result<String> {
    let cancellation = Cancellation(Arc::new(AtomicBool::new(false)));
    let cancelled = cancellation.0.clone();
    provider_setup::offload(move || {
        let queue_deadline = Instant::now() + Duration::from_secs(30);
        let _generation = loop {
            if cancelled.load(Ordering::Relaxed) {
                bail!("Background generation cancelled.");
            }
            if Instant::now() >= queue_deadline {
                bail!("Background generation is busy. Try again shortly.");
            }
            if let Some(guard) = GENERATION.try_lock_for(Duration::from_millis(50)) {
                break guard;
            }
        };
        let server = server(provider_setup::profile_dir()?)?;
        let config = server.background_model()?;
        let (provider, model) = config.split_once('/').context(
            "Select a default model in Settings → Agents → AI providers before using AI features.",
        )?;
        server.generate_text(provider, model, feature, &system, &prompt, &cancelled)
    })
    .await
}

pub async fn model_catalog() -> Result<provider_setup::Snapshot> {
    provider_setup::offload(|| server(provider_setup::profile_dir()?)?.snapshot()).await
}
