use std::fs;

use anyhow::{Context as _, Result};
use uuid::Uuid;

use crate::server::block::Block;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(super) async fn save(block: &Block, title: Option<String>) -> Result<String> {
    let value = serde_json::to_value(block)?;
    warp_core::provider_setup::offload(move || {
        let mut block: Block = serde_json::from_value(value)?;
        let id = Uuid::new_v4().to_string();
        block.id = Some(id.clone());
        let folder = warp_core::provider_setup::profile_dir()?.join("shared-blocks");
        fs::create_dir_all(&folder)?;
        let title = escape(title.as_deref().filter(|t| !t.trim().is_empty()).unwrap_or("MyWarp block"));
        let command = escape(block.command.as_deref().unwrap_or_default());
        let output = escape(block.output.as_deref().unwrap_or_default());
        let html = format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>body{{max-width:960px;margin:48px auto;padding:0 24px;font:16px system-ui}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;padding:20px;background:#f4f4f4;border-radius:8px}}</style><h1>{title}</h1><pre>{command}</pre><pre>{output}</pre></html>");
        let file = folder.join(format!("{id}.html"));
        fs::write(&file, html)?;
        fs::write(folder.join(format!("{id}.json")), serde_json::to_vec(&block)?)?;
        url::Url::from_file_path(file).map(String::from).map_err(|_| anyhow::anyhow!("The exported block path is unavailable"))
    }).await
}

pub(super) async fn list() -> Result<Vec<Block>> {
    warp_core::provider_setup::offload(|| {
        let folder = warp_core::provider_setup::profile_dir()?.join("shared-blocks");
        if !folder.exists() {
            return Ok(vec![]);
        }
        let mut blocks = Vec::new();
        for entry in fs::read_dir(folder)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                blocks.push(serde_json::from_slice::<Block>(&fs::read(path)?)?);
            }
        }
        blocks.sort_by_key(|block| std::cmp::Reverse(block.time_started_term));
        Ok(blocks)
    })
    .await
}

pub(super) async fn remove(id: String) -> Result<()> {
    let id = Uuid::parse_str(&id)
        .context("Invalid local block identifier")?
        .to_string();
    warp_core::provider_setup::offload(move || {
        let folder = warp_core::provider_setup::profile_dir()?.join("shared-blocks");
        for extension in ["html", "json"] {
            let path = folder.join(format!("{id}.{extension}"));
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
#[path = "local_blocks_tests.rs"]
mod tests;
