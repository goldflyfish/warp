//! Offline transcription using installed Windows speech recognizers.
use std::io::Write as _;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use instant::Instant;

const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
Add-Type -AssemblyName System.Speech
$recognizers = @([System.Speech.Recognition.SpeechRecognitionEngine]::InstalledRecognizers())
if ($recognizers.Count -eq 0) { throw 'No Windows speech recognizer is installed.' }
$language = $env:MYWARP_SPEECH_LANGUAGE
$selected = $recognizers | Where-Object { $_.Culture.Name -eq $language } | Select-Object -First 1
if (!$selected -and $language) {
    $selected = $recognizers | Where-Object { $_.Culture.TwoLetterISOLanguageName -eq $language.Split('-')[0] } | Select-Object -First 1
}
if (!$language) { $selected = $recognizers[0] }
if (!$selected) { throw 'Install a Windows speech language pack for the selected language.' }
$engine = New-Object System.Speech.Recognition.SpeechRecognitionEngine($selected)
try {
    $engine.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
    $engine.SetInputToWaveFile($env:MYWARP_SPEECH_INPUT)
    $text = New-Object System.Collections.Generic.List[string]
    while ($null -ne ($result = $engine.Recognize())) { $text.Add($result.Text) }
    @{ text = ($text -join ' ') } | ConvertTo-Json -Compress
} finally { $engine.Dispose() }
"#;

pub async fn transcribe(audio: String, language: Option<String>) -> Result<String> {
    struct Cancel(Arc<AtomicBool>);
    impl Drop for Cancel {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let cancel = Cancel(Arc::new(AtomicBool::new(false)));
    let cancelled = cancel.0.clone();
    crate::provider_setup::offload(move || {
        transcribe_inner(&audio, language.as_deref(), &cancelled)
    })
    .await
}

fn transcribe_inner(audio: &str, language: Option<&str>, cancelled: &AtomicBool) -> Result<String> {
    if audio.len() > 40_000_000 {
        bail!("The voice recording is too large.");
    }
    let wav = STANDARD
        .decode(audio)
        .context("The voice recording is not valid base64")?;
    if !wav.starts_with(b"RIFF") || wav.get(8..12) != Some(b"WAVE") {
        bail!("Voice input requires a WAV recording.");
    }
    let mut file = tempfile::Builder::new().suffix(".wav").tempfile()?;
    file.write_all(&wav)?;
    file.flush()?;
    let path = file.into_temp_path();
    let script: Vec<_> = SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let windows =
        std::env::var_os("SystemRoot").context("The Windows system directory is unavailable")?;
    let executable =
        std::path::PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = command::blocking::Command::new(executable);
    command.creation_flags(0x08000000);
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &STANDARD.encode(script),
        ])
        .env("MYWARP_SPEECH_INPUT", &path)
        .env("MYWARP_SPEECH_LANGUAGE", language.unwrap_or_default())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .context("Windows speech recognition could not start")?;
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Voice transcription was cancelled or timed out.");
        }
        if child.try_wait()?.is_some() {
            break;
        }
        std::thread::park_timeout(Duration::from_millis(50));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        bail!(
            "Windows speech recognition failed. Select an installed speech language in Warp Agent behavior → Voice, or install its Windows speech language pack."
        );
    }
    let result: serde_json::Value = serde_json::from_slice(&output.stdout)
        .context("Windows speech recognition returned an unreadable result")?;
    Ok(result["text"].as_str().unwrap_or_default().to_owned())
}

#[cfg(test)]
#[path = "local_speech_tests.rs"]
mod tests;
