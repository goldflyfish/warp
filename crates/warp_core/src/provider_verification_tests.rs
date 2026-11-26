use super::*;

fn fixture() -> (tempfile::TempDir, Value, Value) {
    let profile = tempfile::tempdir().unwrap();
    let directory = windows_package::configuration_dir(profile.path()).join("providers");
    fs::create_dir_all(directory.join("keys")).unwrap();
    fs::write(directory.join("keys/fixture.key"), "synthetic-private-key").unwrap();
    let config = json!({"model":"fixture/first", "provider":{
        "fixture":{"options":{"apiKey":"{file:./keys/fixture.key}","baseURL":"https://example.com/v1"}}
    }});
    let providers =
        json!({"all":[{"id":"fixture","models":{"first":{},"second":{}}}],"connected":["fixture"]});
    (profile, config, providers)
}

#[test]
fn verified_models_are_restored_from_disk_without_saving_credentials() {
    let (profile, config, providers) = fixture();
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();

    assert_eq!(
        load(profile.path(), &config, &providers),
        BTreeSet::from([("fixture".into(), "first".into())])
    );
    let saved = fs::read_to_string(
        windows_package::configuration_dir(profile.path()).join("providers/verification.json"),
    )
    .unwrap();
    assert!(!saved.contains("synthetic-private-key"));
    assert!(!saved.contains("example.com"));
}

#[test]
fn default_and_unrelated_settings_preserve_verification() {
    let (profile, mut config, providers) = fixture();
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();
    config["model"] = json!("other-provider/second");
    config["provider"]["other-provider"] = json!({"options":{"apiKey":"changed-other-key"}});
    config["provider"]["fixture"]["models"] = json!({"second":{"name":"Another model"}});
    config["permission"] = json!("ask");
    config["mcp"] = json!({});

    assert_eq!(load(profile.path(), &config, &providers).len(), 1);
}

#[test]
fn rotated_key_requires_a_new_test() {
    let (profile, config, providers) = fixture();
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();
    fs::write(
        windows_package::configuration_dir(profile.path()).join("providers/keys/fixture.key"),
        "replacement-key",
    )
    .unwrap();

    assert!(load(profile.path(), &config, &providers).is_empty());
}

#[test]
fn changed_endpoint_requires_a_new_test() {
    let (profile, mut config, providers) = fixture();
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();
    config["provider"]["fixture"]["options"]["baseURL"] = json!("https://other.example/v1");

    assert!(load(profile.path(), &config, &providers).is_empty());
}

#[test]
fn disconnected_or_removed_models_are_not_verified() {
    let (profile, config, mut providers) = fixture();
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();
    providers["connected"] = json!([]);
    assert!(load(profile.path(), &config, &providers).is_empty());
    providers["connected"] = json!(["fixture"]);
    providers["all"][0]["models"] = json!({});
    assert!(load(profile.path(), &config, &providers).is_empty());
}

#[test]
fn failed_retest_only_removes_the_tested_model() {
    let (profile, config, providers) = fixture();
    let first = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    let second = fingerprint(
        profile.path(),
        "fixture",
        "second",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(first)).unwrap();
    save(profile.path(), "fixture", "second", Some(second)).unwrap();
    save(profile.path(), "fixture", "first", None).unwrap();

    assert_eq!(
        load(profile.path(), &config, &providers),
        BTreeSet::from([("fixture".into(), "second".into())])
    );
}

#[test]
fn changed_sign_in_requires_a_new_test() {
    let (profile, config, providers) = fixture();
    let auth =
        windows_package::runtime_dir(profile.path()).join("providers/data/opencode/auth.json");
    fs::create_dir_all(auth.parent().unwrap()).unwrap();
    fs::write(
        &auth,
        r#"{"fixture":{"type":"oauth","access":"old-synthetic-token"}}"#,
    )
    .unwrap();
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();
    fs::write(
        auth,
        r#"{"fixture":{"type":"oauth","access":"new-synthetic-token"}}"#,
    )
    .unwrap();

    assert!(load(profile.path(), &config, &providers).is_empty());
}

#[test]
fn damaged_verification_file_does_not_break_provider_settings() {
    let (profile, config, providers) = fixture();
    fs::write(
        windows_package::configuration_dir(profile.path()).join("providers/verification.json"),
        "{",
    )
    .unwrap();

    assert!(load(profile.path(), &config, &providers).is_empty());
    let fingerprint = fingerprint(
        profile.path(),
        "fixture",
        "first",
        &config,
        &providers["all"][0],
    )
    .unwrap();
    save(profile.path(), "fixture", "first", Some(fingerprint)).unwrap();
    assert_eq!(load(profile.path(), &config, &providers).len(), 1);
}
