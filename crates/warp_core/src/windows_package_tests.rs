use std::fs;
use std::process::Stdio;

use tempfile::tempdir;

use super::*;

#[test]
fn portable_profile_follows_the_executable_instead_of_the_current_directory() {
    let directory = tempdir().unwrap();
    let application = directory.path().join("My Warp 日本語");
    fs::create_dir(&application).unwrap();
    fs::write(application.join(PORTABLE_MARKER), "").unwrap();
    let package = detect_package(&application.join("MyWarp.exe"), Some(directory.path())).unwrap();
    assert_eq!(package.profile_dir(), application.join("Configurations"));
}

#[test]
fn installed_profile_uses_local_app_data_instead_of_the_installation_directory() {
    let directory = tempdir().unwrap();
    let application = directory.path().join("Programs");
    let local_data = directory.path().join("AppData").join("Local");
    fs::create_dir(&application).unwrap();
    fs::write(application.join(INSTALLED_MARKER), "").unwrap();
    let package = detect_package(&application.join("MyWarp.exe"), Some(&local_data)).unwrap();
    assert_eq!(
        package.profile_dir(),
        local_data.join("MyWarp/Configurations")
    );
    assert_eq!(package.instance_namespace(), "MyWarpInstalled");
}

#[test]
fn unpackaged_executables_keep_the_existing_storage_behavior() {
    let directory = tempdir().unwrap();
    assert!(
        detect_package(
            &directory.path().join("warp-oss.exe"),
            Some(directory.path())
        )
        .is_none()
    );
}

#[test]
fn a_directory_named_like_a_marker_does_not_enable_portable_mode() {
    let directory = tempdir().unwrap();
    fs::create_dir(directory.path().join(PORTABLE_MARKER)).unwrap();
    assert!(detect_package(&directory.path().join("MyWarp.exe"), Some(directory.path())).is_none());
}

#[test]
fn initializing_the_provider_profile_preserves_existing_configuration() {
    let directory = tempdir().unwrap();
    prepare_opencode_profile(directory.path()).unwrap();
    let config = directory
        .path()
        .join("providers/config/opencode/opencode.jsonc");
    let custom = r#"{"provider":{"fixture":{"options":{"apiKey":"fixture-key"}}}}"#;
    fs::write(&config, custom).unwrap();
    prepare_opencode_profile(directory.path()).unwrap();
    assert_eq!(fs::read_to_string(config).unwrap(), custom);
}

#[test]
fn copied_personal_instructions_follow_the_new_profile_without_changing_external_sources() {
    let directory = tempdir().unwrap();
    let profile = directory.path().join("Configurations");
    prepare_opencode_profile(&profile).unwrap();
    fs::write(profile.join("agent-instructions.md"), "Keep changes small.").unwrap();
    let config = profile.join("providers/config/opencode/opencode.jsonc");
    fs::write(&config, "{\n// Keep this comment\n\"instructions\":[\"C:/Old/MyWarp/Configurations/agent-instructions.md\",\"C:/Project/agent-instructions.md\",\"docs/rules.md\"]\n}\n").unwrap();

    prepare_opencode_profile(&profile).unwrap();

    let text = fs::read_to_string(config).unwrap();
    assert!(text.contains("// Keep this comment"));
    let root = CstRootNode::parse(&text, &ParseOptions::default()).unwrap();
    let instructions = root
        .object_value()
        .unwrap()
        .array_value("instructions")
        .unwrap()
        .elements();
    assert_eq!(
        instructions[0]
            .as_string_lit()
            .unwrap()
            .decoded_value()
            .unwrap(),
        profile.join("agent-instructions.md").to_string_lossy()
    );
    assert_eq!(
        instructions[1]
            .as_string_lit()
            .unwrap()
            .decoded_value()
            .unwrap(),
        "C:/Project/agent-instructions.md"
    );
    assert_eq!(
        instructions[2]
            .as_string_lit()
            .unwrap()
            .decoded_value()
            .unwrap(),
        "docs/rules.md"
    );
}

#[test]
fn invalid_provider_configuration_is_preserved_when_rebasing_personal_instructions() {
    let directory = tempdir().unwrap();
    prepare_opencode_profile(directory.path()).unwrap();
    fs::write(
        directory.path().join("agent-instructions.md"),
        "Keep changes small.",
    )
    .unwrap();
    let config = directory
        .path()
        .join("providers/config/opencode/opencode.jsonc");
    fs::write(&config, "{invalid").unwrap();

    prepare_opencode_profile(directory.path()).unwrap();

    assert_eq!(fs::read_to_string(config).unwrap(), "{invalid");
}

#[test]
fn migrating_a_portable_profile_preserves_settings_and_referenced_keys() {
    let directory = tempdir().unwrap();
    let legacy = directory.path().join("profile");
    let profile = directory.path().join("Configurations");
    prepare_opencode_profile(&legacy).unwrap();
    fs::create_dir_all(legacy.join("config")).unwrap();
    fs::create_dir_all(legacy.join("providers/config/opencode/keys")).unwrap();
    fs::write(legacy.join("config/settings.toml"), "font_size = 16\n").unwrap();
    fs::write(
        legacy.join("providers/config/opencode/keys/test.key"),
        "fixture-key",
    )
    .unwrap();
    fs::write(
        legacy.join("providers/config/opencode/opencode.jsonc"),
        r#"{"provider":{"fixture":{"options":{"apiKey":"{file:./keys/test.key}"}}}}"#,
    )
    .unwrap();
    fs::create_dir(&profile).unwrap();

    migrate_profile(&legacy, &profile).unwrap();
    prepare_opencode_profile(&profile).unwrap();

    assert!(!legacy.exists());
    assert_eq!(
        fs::read_to_string(profile.join("config/settings.toml")).unwrap(),
        "font_size = 16\n"
    );
    assert_eq!(
        fs::read_to_string(profile.join("providers/config/opencode/keys/test.key")).unwrap(),
        "fixture-key"
    );
    assert_eq!(
        fs::read_to_string(profile.join("providers/config/opencode/opencode.jsonc")).unwrap(),
        r#"{"provider":{"fixture":{"options":{"apiKey":"{file:./keys/test.key}"}}}}"#
    );
    assert!(!profile.join(MIGRATION_MARKER).exists());
}

#[test]
fn migrating_an_installed_profile_moves_data_into_its_configurations_subfolder() {
    let directory = tempdir().unwrap();
    let legacy = directory.path().join("MyWarp");
    let profile = legacy.join("Configurations");
    fs::create_dir_all(legacy.join("state")).unwrap();
    fs::write(legacy.join("state/warp.sqlite"), b"fixture-database").unwrap();
    fs::write(
        legacy.join("agent-preferences.json"),
        r#"{"tool_policy":"ask"}"#,
    )
    .unwrap();

    migrate_profile(&legacy, &profile).unwrap();
    migrate_profile(&legacy, &profile).unwrap();

    assert_eq!(
        fs::read(profile.join("state/warp.sqlite")).unwrap(),
        b"fixture-database"
    );
    assert_eq!(
        fs::read_to_string(profile.join("agent-preferences.json")).unwrap(),
        r#"{"tool_policy":"ask"}"#
    );
    assert!(!legacy.join("state").exists());
    assert!(!profile.join("Configurations").exists());
    assert!(!profile.join(MIGRATION_MARKER).exists());
}

#[test]
fn a_copied_configurations_folder_takes_precedence_over_a_legacy_profile() {
    let directory = tempdir().unwrap();
    let legacy = directory.path().join("profile");
    let profile = directory.path().join("Configurations");
    fs::create_dir_all(legacy.join("config")).unwrap();
    fs::create_dir_all(profile.join("config")).unwrap();
    fs::write(legacy.join("config/settings.toml"), "font_size = 12\n").unwrap();
    fs::write(profile.join("config/settings.toml"), "font_size = 18\n").unwrap();

    migrate_profile(&legacy, &profile).unwrap();

    assert_eq!(
        fs::read_to_string(profile.join("config/settings.toml")).unwrap(),
        "font_size = 18\n"
    );
    assert_eq!(
        fs::read_to_string(legacy.join("config/settings.toml")).unwrap(),
        "font_size = 12\n"
    );
}

#[test]
fn an_interrupted_profile_migration_resumes_without_losing_moved_files() {
    let directory = tempdir().unwrap();
    let legacy = directory.path().join("profile");
    let profile = directory.path().join("Configurations");
    fs::create_dir_all(&legacy).unwrap();
    fs::create_dir_all(profile.join("config")).unwrap();
    fs::write(legacy.join("agent-instructions.md"), "Keep changes small.").unwrap();
    fs::write(profile.join("config/settings.toml"), "font_size = 16\n").unwrap();
    fs::write(profile.join(MIGRATION_MARKER), "").unwrap();

    migrate_profile(&legacy, &profile).unwrap();

    assert_eq!(
        fs::read_to_string(profile.join("agent-instructions.md")).unwrap(),
        "Keep changes small."
    );
    assert_eq!(
        fs::read_to_string(profile.join("config/settings.toml")).unwrap(),
        "font_size = 16\n"
    );
    assert!(!legacy.exists());
    assert!(!profile.join(MIGRATION_MARKER).exists());
}

#[test]
fn an_interrupted_profile_migration_never_overwrites_conflicting_files() {
    let directory = tempdir().unwrap();
    let legacy = directory.path().join("profile");
    let profile = directory.path().join("Configurations");
    fs::create_dir_all(&legacy).unwrap();
    fs::create_dir_all(&profile).unwrap();
    fs::write(legacy.join("agent-instructions.md"), "old instructions").unwrap();
    fs::write(profile.join("agent-instructions.md"), "copied instructions").unwrap();
    fs::write(profile.join(MIGRATION_MARKER), "").unwrap();

    assert!(migrate_profile(&legacy, &profile).is_err());

    assert_eq!(
        fs::read_to_string(profile.join("agent-instructions.md")).unwrap(),
        "copied instructions"
    );
    assert_eq!(
        fs::read_to_string(legacy.join("agent-instructions.md")).unwrap(),
        "old instructions"
    );
}

#[test]
fn separate_portable_profiles_have_distinct_single_instance_names() {
    let first = Package::Portable(PathBuf::from("C:\\Apps\\First\\Configurations"));
    let second = Package::Portable(PathBuf::from("C:\\Apps\\Second\\Configurations"));
    assert_ne!(first.instance_namespace(), second.instance_namespace());
    assert_ne!(
        first.instance_namespace(),
        Package::Installed(PathBuf::new()).instance_namespace()
    );
}

#[test]
fn opencode_storage_and_configuration_all_stay_inside_the_package_profile() {
    let root = Path::new("C:\\Apps\\MyWarp\\Configurations");
    let environment = opencode_environment(root);
    assert_eq!(environment.len(), 6);
    assert!(environment.iter().all(|(_, path)| path.starts_with(root)));
    assert!(environment.contains(&(
        "OPENCODE_CONFIG",
        root.join("providers/config/opencode/opencode.jsonc")
    )));
    assert!(environment.contains(&("XDG_DATA_HOME", root.join("providers/data"))));
}

fn check_process_paths(marker: &str, installed_profile: Option<PathBuf>) {
    let directory = tempdir().unwrap();
    let executable = directory.path().join("MyWarp.exe");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    fs::write(directory.path().join(marker), "").unwrap();
    let output = command::blocking::Command::new(executable)
        .args([
            "--exact",
            "windows_package::tests::packaged_process_paths_probe",
            "--ignored",
            "--nocapture",
        ])
        .env(
            "MYWARP_EXPECTED_PROFILE",
            installed_profile.unwrap_or_else(|| {
                fs::canonicalize(directory.path())
                    .unwrap()
                    .join("Configurations")
            }),
        )
        .creation_flags(0)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn portable_process_routes_all_warp_paths_into_its_own_folder() {
    check_process_paths(PORTABLE_MARKER, None);
}

#[test]
fn installed_process_routes_all_warp_paths_into_local_app_data() {
    check_process_paths(
        INSTALLED_MARKER,
        Some(
            BaseDirs::new()
                .unwrap()
                .data_local_dir()
                .join("MyWarp/Configurations"),
        ),
    );
}

#[test]
#[ignore = "subprocess probe invoked by the package isolation tests"]
fn packaged_process_paths_probe() {
    let expected = PathBuf::from(std::env::var_os("MYWARP_EXPECTED_PROFILE").unwrap());
    assert_eq!(profile_dir(), Some(expected.as_path()));
    assert_eq!(crate::paths::config_local_dir(), expected.join("config"));
    assert_eq!(
        crate::paths::gui_config_local_dir(),
        Some(expected.join("config"))
    );
    assert_eq!(
        crate::paths::warp_home_config_dir(),
        Some(expected.join("config"))
    );
    assert_eq!(crate::paths::state_dir(), expected.join("state"));
    assert_eq!(crate::paths::cache_dir(), expected.join("cache"));
    assert_eq!(crate::paths::data_dir(), expected.join("data"));
}

#[test]
fn imports_existing_provider_files_into_the_portable_profile() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("existing config");
    fs::create_dir_all(source.join("keys")).unwrap();
    fs::write(source.join("keys/router.key"), "dummy-provider-key").unwrap();
    fs::write(source.join("opencode.jsonc"), "{\n// existing configuration\n\"provider\":{\"openrouter\":{\"options\":{\"apiKey\":\"{file:./keys/router.key}\"}}}}\n").unwrap();
    fs::write(
        source.join("auth.json"),
        r#"{"openai":{"type":"oauth","refresh":"dummy-token"}}"#,
    )
    .unwrap();
    let profile = temp.path().join("portable profile");
    prepare_opencode_profile(&profile).unwrap();

    import_opencode_configuration(
        &profile,
        Some(&source.join("opencode.jsonc")),
        &source.join("auth.json"),
    )
    .unwrap();

    let config =
        fs::read_to_string(profile.join("providers/config/opencode/opencode.jsonc")).unwrap();
    assert!(config.contains("{file:./keys/imported-0.key}"));
    assert!(!config.contains("dummy-provider-key"));
    assert_eq!(
        fs::read_to_string(profile.join("providers/config/opencode/keys/imported-0.key")).unwrap(),
        "dummy-provider-key"
    );
    assert!(profile.join("providers/data/opencode/auth.json").is_file());
}

#[test]
fn existing_profile_configuration_is_not_replaced_by_global_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    prepare_opencode_profile(&profile).unwrap();
    let destination = profile.join("providers/config/opencode/opencode.jsonc");
    fs::write(&destination, r#"{"provider":{"zai":{}}}"#).unwrap();
    let global = temp.path().join("global.json");
    fs::write(&global, r#"{"provider":{"openrouter":{}}}"#).unwrap();

    import_opencode_configuration(
        &profile,
        Some(&global),
        &temp.path().join("missing-auth.json"),
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        r#"{"provider":{"zai":{}}}"#
    );
}

#[test]
fn missing_referenced_credentials_leave_the_original_profile_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    prepare_opencode_profile(&profile).unwrap();
    let destination = profile.join("providers/config/opencode/opencode.jsonc");
    let original = fs::read(&destination).unwrap();
    let global = temp.path().join("global.json");
    fs::write(
        &global,
        r#"{"provider":{"openrouter":{"options":{"apiKey":"{file:./missing.key}"}}}}"#,
    )
    .unwrap();

    assert!(
        import_opencode_configuration(
            &profile,
            Some(&global),
            &temp.path().join("missing-auth.json")
        )
        .is_err()
    );
    assert_eq!(fs::read(destination).unwrap(), original);
}

#[test]
fn imports_provider_signins_without_overwriting_custom_profile_settings() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    prepare_opencode_profile(&profile).unwrap();
    let config = profile.join("providers/config/opencode/opencode.jsonc");
    fs::write(&config, r#"{"provider":{"zai":{}}}"#).unwrap();
    let auth = temp.path().join("auth.json");
    fs::write(
        &auth,
        r#"{"openai":{"type":"oauth","refresh":"dummy-token"}}"#,
    )
    .unwrap();
    import_opencode_configuration(&profile, None, &auth).unwrap();
    assert_eq!(
        fs::read_to_string(config).unwrap(),
        r#"{"provider":{"zai":{}}}"#
    );
    assert_eq!(
        fs::read(profile.join("providers/data/opencode/auth.json")).unwrap(),
        fs::read(auth).unwrap()
    );
}
