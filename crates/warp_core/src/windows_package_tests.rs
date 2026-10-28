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
    assert_eq!(package.profile_dir(), application);
}

#[test]
fn installed_profile_uses_local_app_data_instead_of_the_installation_directory() {
    let directory = tempdir().unwrap();
    let application = directory.path().join("Programs");
    let local_data = directory.path().join("AppData/Local");
    fs::create_dir(&application).unwrap();
    fs::write(application.join(INSTALLED_MARKER), "").unwrap();
    let package = detect_package(&application.join("MyWarp.exe"), Some(&local_data)).unwrap();
    assert_eq!(package.profile_dir(), local_data.join("MyWarp"));
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
    let config = provider_configuration_file(directory.path());
    let custom = r#"{"provider":{"fixture":{"options":{"apiKey":"fixture-key"}}}}"#;
    fs::write(&config, custom).unwrap();
    prepare_opencode_profile(directory.path()).unwrap();
    assert_eq!(fs::read_to_string(config).unwrap(), custom);
}

#[test]
fn old_mixed_configurations_are_split_into_pure_files_and_runtime_data() {
    let directory = tempdir().unwrap();
    let old = directory.path().join("Configurations");
    fs::create_dir_all(old.join("config")).unwrap();
    fs::create_dir_all(old.join("data/themes")).unwrap();
    fs::create_dir_all(old.join("state")).unwrap();
    fs::create_dir_all(old.join("providers/config/opencode/keys")).unwrap();
    fs::create_dir_all(old.join("providers/config/opencode/node_modules")).unwrap();
    fs::create_dir_all(old.join("providers/data/opencode")).unwrap();
    fs::write(
        old.join("config/settings.toml"),
        "[agents.execution_profiles.default]\nname = \"My profile\"\n",
    )
    .unwrap();
    fs::write(old.join("data/themes/custom.yaml"), "name: Custom\n").unwrap();
    fs::write(old.join("state/warp.sqlite"), "history").unwrap();
    fs::write(old.join("providers/config/opencode/opencode.jsonc"), r#"{"model":"fixture/model","provider":{"fixture":{"options":{"apiKey":"{file:./keys/fixture.key}"}}}}"#).unwrap();
    fs::write(
        old.join("providers/config/opencode/keys/fixture.key"),
        "synthetic-key",
    )
    .unwrap();
    fs::write(
        old.join("providers/config/opencode/node_modules/runtime.bin"),
        "downloaded code",
    )
    .unwrap();
    fs::write(old.join("providers/data/opencode/auth.json"), "{}").unwrap();

    prepare_opencode_profile(directory.path()).unwrap();
    prepare_opencode_profile(directory.path()).unwrap();

    assert_eq!(
        fs::read_to_string(old.join("settings.toml")).unwrap(),
        "[agents.execution_profiles.default]\nname = \"My profile\"\n"
    );
    assert!(old.join("themes/custom.yaml").is_file());
    assert!(old.join("providers/keys/fixture.key").is_file());
    assert!(directory.path().join("Runtime/state/warp.sqlite").is_file());
    assert!(
        directory
            .path()
            .join("Runtime/providers/data/opencode/auth.json")
            .is_file()
    );
    assert!(
        directory
            .path()
            .join("Runtime/legacy-provider-config/opencode/node_modules/runtime.bin")
            .is_file()
    );
    assert!(!old.join("state").exists());
    assert!(!old.join("data").exists());
    assert!(!old.join("config").exists());
    assert!(!old.join("providers/config").exists());
    assert!(!old.join("providers/data").exists());
}

#[test]
fn copying_only_configuration_preserves_profiles_themes_and_provider_keys_without_history() {
    let original = tempdir().unwrap();
    let copied = tempdir().unwrap();
    prepare_opencode_profile(original.path()).unwrap();
    let config = configuration_dir(original.path());
    fs::write(
        config.join("settings.toml"),
        "[agents.execution_profiles.default]\nname = \"Portable profile\"\n",
    )
    .unwrap();
    fs::write(config.join("themes/my-theme.yaml"), "name: My Theme\n").unwrap();
    fs::create_dir_all(config.join("providers/keys")).unwrap();
    fs::write(config.join("providers/keys/test.key"), "synthetic-key").unwrap();
    fs::write(
        provider_configuration_file(original.path()),
        r#"{"provider":{"fixture":{"options":{"apiKey":"{file:./keys/test.key}"}}}}"#,
    )
    .unwrap();
    fs::write(
        original.path().join("Runtime/state/history.sqlite"),
        "do not copy",
    )
    .unwrap();

    copy_configuration(&config, &configuration_dir(copied.path())).unwrap();
    prepare_opencode_profile(copied.path()).unwrap();

    let destination = configuration_dir(copied.path());
    assert_eq!(
        fs::read_to_string(destination.join("settings.toml")).unwrap(),
        "[agents.execution_profiles.default]\nname = \"Portable profile\"\n"
    );
    assert_eq!(
        fs::read_to_string(destination.join("providers/keys/test.key")).unwrap(),
        "synthetic-key"
    );
    assert!(destination.join("themes/my-theme.yaml").is_file());
    assert!(!copied.path().join("Runtime/state/history.sqlite").exists());
    assert!(!destination.join("Runtime").exists());
    assert!(!destination.join("state").exists());
}

#[test]
fn a_copied_configurations_folder_takes_precedence_over_a_legacy_profile() {
    let directory = tempdir().unwrap();
    let legacy = directory.path().join("profile");
    let config = configuration_dir(directory.path());
    fs::create_dir_all(legacy.join("config")).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(legacy.join("config/settings.toml"), "font_size = 12\n").unwrap();
    fs::write(config.join("settings.toml"), "font_size = 18\n").unwrap();

    prepare_opencode_profile(directory.path()).unwrap();

    assert_eq!(
        fs::read_to_string(config.join("settings.toml")).unwrap(),
        "font_size = 18\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("Runtime/config/settings.toml")).unwrap(),
        "font_size = 12\n"
    );
    assert!(!legacy.exists());
}

#[test]
fn shipped_defaults_do_not_hide_existing_settings_during_migration() {
    let directory = tempdir().unwrap();
    fs::create_dir_all(directory.path().join("profile/config")).unwrap();
    fs::write(
        directory.path().join("profile/config/settings.toml"),
        "font_size = 19\n",
    )
    .unwrap();
    fs::create_dir_all(configuration_dir(directory.path())).unwrap();
    fs::write(
        configuration_dir(directory.path()).join("settings.toml"),
        DEFAULT_SETTINGS,
    )
    .unwrap();

    prepare_opencode_profile(directory.path()).unwrap();

    assert_eq!(
        fs::read_to_string(configuration_dir(directory.path()).join("settings.toml")).unwrap(),
        "font_size = 19\n"
    );
}

#[test]
fn personal_instructions_use_a_portable_reference_and_preserve_external_sources() {
    let directory = tempdir().unwrap();
    prepare_opencode_profile(directory.path()).unwrap();
    fs::write(
        configuration_dir(directory.path()).join("agent-instructions.md"),
        "Keep changes small.",
    )
    .unwrap();
    let config = provider_configuration_file(directory.path());
    fs::write(&config, "{\n// Keep this comment\n\"instructions\":[\"C:/Old/MyWarp/Configurations/agent-instructions.md\",\"C:/Project/agent-instructions.md\"]\n}\n").unwrap();

    prepare_opencode_profile(directory.path()).unwrap();

    let text = fs::read_to_string(config).unwrap();
    assert!(text.contains("// Keep this comment"));
    assert!(text.contains("{env:MYWARP_CONFIGURATIONS_DIR}/agent-instructions.md"));
    assert!(text.contains("C:/Project/agent-instructions.md"));
    assert!(!text.contains("C:/Old/MyWarp"));
}

#[test]
fn separate_portable_profiles_have_distinct_single_instance_names() {
    let first = Package::Portable(PathBuf::from("C:\\Apps\\First"));
    let second = Package::Portable(PathBuf::from("C:\\Apps\\Second"));
    assert_ne!(first.instance_namespace(), second.instance_namespace());
    assert_ne!(
        first.instance_namespace(),
        Package::Installed(PathBuf::new()).instance_namespace()
    );
}

#[test]
fn runtime_dependencies_and_auth_never_use_the_configuration_directory() {
    let root = Path::new("C:\\Apps\\MyWarp");
    let environment = opencode_environment(root);
    assert!(environment.contains(&(
        "OPENCODE_CONFIG",
        root.join("Configurations/providers/opencode.jsonc")
    )));
    assert!(environment.contains(&(
        "OPENCODE_CONFIG_DIR",
        root.join("Runtime/providers/config/opencode")
    )));
    assert!(environment.contains(&("XDG_DATA_HOME", root.join("Runtime/providers/data"))));
    assert!(environment.contains(&("MYWARP_CONFIGURATIONS_DIR", root.join("Configurations"))));
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
            installed_profile.unwrap_or_else(|| fs::canonicalize(directory.path()).unwrap()),
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
fn portable_process_routes_files_into_separate_configuration_and_runtime_folders() {
    check_process_paths(PORTABLE_MARKER, None);
}

#[test]
fn installed_process_routes_files_into_local_app_data() {
    check_process_paths(
        INSTALLED_MARKER,
        Some(BaseDirs::new().unwrap().data_local_dir().join("MyWarp")),
    );
}

#[test]
#[ignore = "subprocess probe invoked by the package isolation tests"]
fn packaged_process_paths_probe() {
    let expected = PathBuf::from(std::env::var_os("MYWARP_EXPECTED_PROFILE").unwrap());
    assert_eq!(profile_dir(), Some(expected.as_path()));
    assert_eq!(
        crate::paths::config_local_dir(),
        expected.join("Configurations")
    );
    assert_eq!(
        crate::paths::gui_config_local_dir(),
        Some(expected.join("Configurations"))
    );
    assert_eq!(
        crate::paths::warp_home_config_dir(),
        Some(expected.join("Configurations"))
    );
    assert_eq!(
        crate::paths::themes_dir(),
        expected.join("Configurations/themes")
    );
    assert_eq!(
        crate::paths::user_config_dir(),
        expected.join("Configurations")
    );
    assert_eq!(crate::paths::state_dir(), expected.join("Runtime/state"));
    assert_eq!(crate::paths::cache_dir(), expected.join("Runtime/cache"));
    assert_eq!(crate::paths::data_dir(), expected.join("Runtime/data"));
    assert_eq!(
        crate::paths::private_config_local_dir(),
        expected.join("Runtime/config")
    );
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
        fs::read_to_string(profile.join("Configurations/providers/opencode.jsonc")).unwrap();
    assert!(config.contains("{file:./keys/imported-0.key}"));
    assert!(!config.contains("dummy-provider-key"));
    assert_eq!(
        fs::read_to_string(profile.join("Configurations/providers/keys/imported-0.key")).unwrap(),
        "dummy-provider-key"
    );
    assert!(
        profile
            .join("Runtime/providers/data/opencode/auth.json")
            .is_file()
    );
}

#[test]
fn existing_profile_configuration_is_not_replaced_by_global_configuration() {
    let temp = tempfile::tempdir().unwrap();
    let profile = temp.path().join("profile");
    prepare_opencode_profile(&profile).unwrap();
    let destination = profile.join("Configurations/providers/opencode.jsonc");
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
    let destination = profile.join("Configurations/providers/opencode.jsonc");
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
    let config = profile.join("Configurations/providers/opencode.jsonc");
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
        fs::read(profile.join("Runtime/providers/data/opencode/auth.json")).unwrap(),
        fs::read(auth).unwrap()
    );
}
