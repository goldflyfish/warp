use std::fs;

use super::revision;

#[test]
fn revision_tracks_key_contents_models_auth_and_preferences() {
    let profile = tempfile::tempdir().unwrap();
    crate::windows_package::prepare_opencode_profile(profile.path()).unwrap();
    let config = crate::provider_setup::configuration_file(profile.path());
    let key = config.parent().unwrap().join("test.key");
    fs::write(&key, "first-key").unwrap();
    fs::write(&config, r#"{// comment
        "provider":{"fixture":{"options":{"apiKey":"{file:./test.key}"}}},"model":"fixture/first"}"#).unwrap();
    let first = revision(profile.path()).unwrap();
    assert_eq!(first, revision(profile.path()).unwrap());
    fs::write(&key, "other-key").unwrap();
    let changed_key = revision(profile.path()).unwrap();
    assert_ne!(first, changed_key);
    fs::write(&config, "{\"model\":\"fixture/second\"}").unwrap();
    let changed_model = revision(profile.path()).unwrap();
    assert_ne!(changed_key, changed_model);
    let auth = profile
        .path()
        .join("Runtime/providers/data/opencode/auth.json");
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    fs::write(auth, r#"{"fixture":{"type":"api","key":"synthetic"}}"#).unwrap();
    let changed_auth = revision(profile.path()).unwrap();
    assert_ne!(changed_model, changed_auth);
    fs::write(
        profile.path().join("Configurations/agent-preferences.json"),
        r#"{"tool_policy":"readonly"}"#,
    )
    .unwrap();
    assert_ne!(changed_auth, revision(profile.path()).unwrap());
}

#[test]
fn incomplete_or_invalid_edits_do_not_produce_a_reload_revision() {
    let profile = tempfile::tempdir().unwrap();
    crate::windows_package::prepare_opencode_profile(profile.path()).unwrap();
    let config = crate::provider_setup::configuration_file(profile.path());
    let valid = revision(profile.path()).unwrap();
    fs::write(&config, "{\"provider\":").unwrap();
    assert!(revision(profile.path()).is_err());
    fs::write(
        &config,
        r#"{"provider":{"fixture":{"options":{"apiKey":"{file:./missing.key}"}}}}"#,
    )
    .unwrap();
    assert!(revision(profile.path()).is_err());
    fs::write(
        &config,
        r#"{"$schema":"https://opencode.ai/config.json"}
"#,
    )
    .unwrap();
    assert_eq!(valid, revision(profile.path()).unwrap());
}
