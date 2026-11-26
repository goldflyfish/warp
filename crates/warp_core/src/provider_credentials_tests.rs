use super::*;

fn profile() -> tempfile::TempDir {
    let profile = tempfile::tempdir().unwrap();
    windows_package::prepare_opencode_profile(profile.path()).unwrap();
    profile
}

#[test]
fn saved_key_is_encrypted_and_can_be_unlocked() {
    let profile = profile();
    let path = save_key(profile.path(), "synthetic-provider-secret").unwrap();

    let saved = fs::read_to_string(&path).unwrap();
    assert!(!saved.contains("synthetic-provider-secret"));
    assert_eq!(read_key(&path).unwrap(), "synthetic-provider-secret");
}

#[test]
fn corrupt_ciphertext_stops_runtime_startup_without_plaintext_fallback() {
    let profile = profile();
    let path = save_key(profile.path(), "synthetic-provider-secret").unwrap();
    fs::write(&path, "MYWARP-DPAPI-V1\nAAAA").unwrap();
    fs::write(
        windows_package::provider_configuration_file(profile.path()),
        json!({"provider":{"fixture":{"options":{"apiKey":reference(&path)}}}}).to_string(),
    )
    .unwrap();

    assert!(runtime_overrides(profile.path()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "MYWARP-DPAPI-V1\nAAAA");
}

#[test]
fn unsupported_encryption_versions_are_not_treated_as_plaintext() {
    let profile = profile();
    let path = save_key(profile.path(), "synthetic-provider-secret").unwrap();
    fs::write(&path, "MYWARP-DPAPI-V2\nunsupported").unwrap();
    fs::write(
        windows_package::provider_configuration_file(profile.path()),
        json!({"provider":{"fixture":{"options":{"apiKey":reference(&path)}}}}).to_string(),
    )
    .unwrap();

    assert!(runtime_overrides(profile.path()).is_err());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "MYWARP-DPAPI-V2\nunsupported"
    );
}

#[test]
fn inline_key_migration_preserves_comments_and_only_unseals_for_runtime() {
    let profile = profile();
    let path = windows_package::provider_configuration_file(profile.path());
    fs::write(&path, "{\n// keep provider notes\n\"provider\":{\"fixture\":{\"options\":{\"apiKey\":\"synthetic-inline-secret\",\"baseURL\":\"https://example.com\"}}}}\n").unwrap();

    let runtime = runtime_overrides(profile.path()).unwrap();

    assert_eq!(
        runtime["provider"]["fixture"]["options"]["apiKey"],
        "synthetic-inline-secret"
    );
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("keep provider notes"));
    assert!(!saved.contains("synthetic-inline-secret"));
    assert_eq!(saved.matches("\"apiKey\"").count(), 1);
    protect_profile(profile.path()).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), saved);
}

#[test]
fn referenced_profile_key_is_encrypted_in_place() {
    let profile = profile();
    let path = windows_package::configuration_dir(profile.path()).join("providers/legacy.key");
    fs::write(&path, "synthetic-file-secret\n").unwrap();
    let config = windows_package::provider_configuration_file(profile.path());
    fs::write(
        &config,
        r#"{"provider":{"fixture":{"options":{"apiKey":"{file:./legacy.key}"}}}}"#,
    )
    .unwrap();

    let runtime = runtime_overrides(profile.path()).unwrap();

    assert_eq!(
        runtime["provider"]["fixture"]["options"]["apiKey"],
        "synthetic-file-secret"
    );
    assert!(
        !fs::read_to_string(&path)
            .unwrap()
            .contains("synthetic-file-secret")
    );
    let encrypted = fs::read(&path).unwrap();
    protect_profile(profile.path()).unwrap();
    assert_eq!(fs::read(path).unwrap(), encrypted);
}

#[test]
fn external_key_is_copied_into_encrypted_storage_without_modifying_source() {
    let profile = profile();
    let external = tempfile::tempdir().unwrap();
    let path = external.path().join("external.key");
    fs::write(&path, "synthetic-external-secret").unwrap();
    fs::write(windows_package::provider_configuration_file(profile.path()),
        json!({"provider":{"fixture":{"options":{"apiKey":format!("{{file:{}}}", path.display())}}}}).to_string()).unwrap();

    let runtime = runtime_overrides(profile.path()).unwrap();

    assert_eq!(
        runtime["provider"]["fixture"]["options"]["apiKey"],
        "synthetic-external-secret"
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "synthetic-external-secret"
    );
    assert!(
        !fs::read_to_string(windows_package::provider_configuration_file(profile.path()))
            .unwrap()
            .contains("external.key")
    );
}

#[test]
fn imported_api_auth_is_migrated_and_oauth_signin_is_preserved() {
    let profile = profile();
    let auth =
        windows_package::runtime_dir(profile.path()).join("providers/data/opencode/auth.json");
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    fs::write(&auth, r#"{"fixture":{"type":"api","key":"synthetic-auth-secret"},"signin":{"type":"oauth","refresh":"synthetic-refresh"}}"#).unwrap();

    let runtime = runtime_overrides(profile.path()).unwrap();

    assert_eq!(
        runtime["provider"]["fixture"]["options"]["apiKey"],
        "synthetic-auth-secret"
    );
    let saved: Value = serde_json::from_slice(&fs::read(auth).unwrap()).unwrap();
    assert!(saved.get("fixture").is_none());
    assert_eq!(saved["signin"]["refresh"], "synthetic-refresh");
}

#[test]
fn configured_key_takes_precedence_over_imported_api_auth() {
    let profile = profile();
    fs::write(
        windows_package::provider_configuration_file(profile.path()),
        r#"{"provider":{"fixture":{"options":{"apiKey":"configured-secret"}}}}"#,
    )
    .unwrap();
    let auth =
        windows_package::runtime_dir(profile.path()).join("providers/data/opencode/auth.json");
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    fs::write(&auth, r#"{"fixture":{"type":"api","key":"older-secret"}}"#).unwrap();

    let runtime = runtime_overrides(profile.path()).unwrap();

    assert_eq!(
        runtime["provider"]["fixture"]["options"]["apiKey"],
        "configured-secret"
    );
    assert!(!fs::read_to_string(auth).unwrap().contains("older-secret"));
}

#[test]
fn environment_keys_remain_external_to_saved_configuration() {
    let profile = profile();
    let path = windows_package::provider_configuration_file(profile.path());
    let config =
        r#"{"provider":{"fixture":{"options":{"apiKey":"{env:MYWARP_TEST_PROVIDER_KEY}"}}}}"#;
    fs::write(&path, config).unwrap();

    assert_eq!(runtime_overrides(profile.path()).unwrap(), json!({}));
    assert_eq!(fs::read_to_string(path).unwrap(), config);
}
