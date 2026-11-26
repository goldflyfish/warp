use std::sync::atomic::AtomicBool;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::transcribe_inner;

#[test]
fn rejects_non_wav_recordings_before_starting_a_process() {
    assert!(
        transcribe_inner(
            &STANDARD.encode(b"not audio"),
            None,
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .to_string()
        .contains("WAV")
    );
    assert!(transcribe_inner("not base64!", None, &AtomicBool::new(false)).is_err());
}

#[test]
#[ignore = "requires an installed Windows English speech recognizer and synthesis voice"]
fn windows_offline_speech_transcribes_a_synthetic_recording() {
    use std::fs;
    use std::process::Stdio;
    let folder = tempfile::tempdir().unwrap();
    let wav = folder.path().join("synthetic.wav");
    let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = command::blocking::Command::new(executable);
    command.creation_flags(0x08000000);
    let output = command.args(["-NoProfile","-NonInteractive","-Command", "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.SetOutputToWaveFile($env:MYWARP_TEST_WAV); $s.Speak('open the terminal and show the current directory'); $s.Dispose()"])
        .env("MYWARP_TEST_WAV", &wav).stdin(Stdio::null()).output().unwrap();
    assert!(output.status.success());
    let text = transcribe_inner(
        &STANDARD.encode(fs::read(wav).unwrap()),
        Some("en"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        !text.trim().is_empty(),
        "Offline transcription should recognize the synthetic phrase"
    );
}
