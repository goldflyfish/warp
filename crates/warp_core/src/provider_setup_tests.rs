use super::*;

#[test]
fn provider_setup_snapshot_keeps_keys_and_tokens_out_of_ui_state() {
    let providers = json!({"all":[{"id":"openrouter","name":"OpenRouter","models":{"model/id":{"name":"Model"}}}],"connected":["openrouter"]});
    let config = json!({"provider":{"openrouter":{"options":{"apiKey":"do-not-show-this-secret","baseURL":"https://example.com/v1"}}},"model":"openrouter/model/id"});
    let state = snapshot(&providers, &config, &json!({})).unwrap();
    assert_eq!(
        state.providers[0].credential,
        "API key saved in this profile"
    );
    assert_eq!(state.providers[0].models["model/id"], "Model");
    assert_eq!(state.default_model, "openrouter/model/id");
    assert!(state.providers[0].connected);
}

#[test]
fn provider_setup_endpoint_rejects_credentials_in_urls_and_remote_plain_http() {
    for endpoint in [
        "http://example.com/v1",
        "https://secret@example.com/v1",
        "https://example.com?key=secret",
        "https://app.warp.dev",
    ] {
        assert!(validate_endpoint(endpoint).is_err(), "{endpoint}");
    }
    for endpoint in [
        "",
        "https://api.example.com/v1",
        "http://127.0.0.1:11434/v1",
    ] {
        assert!(validate_endpoint(endpoint).is_ok(), "{endpoint}");
    }
}

#[test]
fn provider_setup_identifier_cannot_escape_auth_routes_or_key_folder() {
    for id in ["../openai", "openai?key=x", "a/b", ""] {
        assert!(validate_provider_id(id).is_err());
    }
    assert!(validate_provider_id("zai-coding-plan").is_ok());
}

#[test]
fn provider_setup_permissions_enforce_read_only_and_review_policy() {
    assert_eq!(tool_permissions("ask").unwrap(), json!("ask"));
    let read_only = tool_permissions("readonly").unwrap();
    assert_eq!(read_only["*"], "deny");
    assert_eq!(read_only["read"], "allow");
    assert_eq!(tool_permissions("review").unwrap()["*"], "ask");
    assert!(tool_permissions("allow-everything").is_err());
}

#[test]
fn provider_setup_chat_loads_saved_behavior_and_fails_closed_on_invalid_policy() {
    let profile = tempfile::tempdir().unwrap();
    windows_package::prepare_opencode_profile(profile.path()).unwrap();
    assert_eq!(
        runtime_overrides(profile.path()).unwrap()["permission"],
        "ask"
    );
    fs::write(
        profile.path().join("Configurations/agent-preferences.json"),
        r#"{"tool_policy":"readonly","default_agent":"plan"}"#,
    )
    .unwrap();
    let config = runtime_overrides(profile.path()).unwrap();
    assert_eq!(config["permission"]["*"], "deny");
    assert_eq!(config["default_agent"], "plan");
    fs::write(
        profile.path().join("Configurations/agent-preferences.json"),
        r#"{"tool_policy":"allow-everything"}"#,
    )
    .unwrap();
    assert!(runtime_overrides(profile.path()).is_err());
}

#[test]
fn provider_setup_oauth_only_offers_methods_the_form_can_complete() {
    let providers = json!({"all":[{"id":"openai","name":"OpenAI","models":{}}],"connected":[]});
    let methods = json!({"openai":[{"type":"oauth","label":"Browser"},{"type":"api","label":"API Key"},{"type":"oauth","label":"Needs extra input","prompts":[{}]}]});
    let state = snapshot(&providers, &json!({}), &methods).unwrap();
    assert_eq!(state.providers[0].oauth, vec![(0, "Browser".into())]);
}

#[cfg(feature = "test-util")]
#[test]
#[ignore = "requires the bundled OpenCode executable in MYWARP_TEST_OPENCODE"]
fn provider_setup_real_runtime_saves_tests_rotates_and_disconnects_api_keys() {
    let executable = std::env::var_os("MYWARP_TEST_OPENCODE")
        .expect("Set MYWARP_TEST_OPENCODE to the bundled executable");
    let mut fixture = mockito::Server::new();
    let stream = concat!(
        "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"MYWARP_CONNECTION_OK\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3,\"total_tokens\":8}}\n\n",
        "data: [DONE]\n\n"
    );
    let success = fixture
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer fixture-valid-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(stream)
        .expect_at_least(1)
        .create();
    let rejected=fixture.mock("POST","/v1/chat/completions").match_header("authorization","Bearer fixture-invalid-key")
        .with_status(401).with_header("content-type","application/json").with_body(r#"{"error":{"message":"do-not-expose-provider-response-secret","type":"invalid_api_key"}}"#).expect_at_least(1).create();
    let profile = tempfile::tempdir().unwrap();
    windows_package::prepare_opencode_profile(profile.path()).unwrap();
    fs::write(
        profile
            .path()
            .join("Configurations/providers/opencode.jsonc"),
        "{\n// keep this comment\n\"$schema\":\"https://opencode.ai/config.json\"\n}\n",
    )
    .unwrap();
    let server =
        ProviderServer::start_inner(profile.path().into(), PathBuf::from(executable), false)
            .unwrap();
    let endpoint = format!("{}/v1", fixture.url());
    let saved = server
        .save_provider(
            "custom-fixture",
            "fixture-valid-key",
            &endpoint,
            "fixture-model",
        )
        .unwrap();
    assert!(
        saved
            .providers
            .iter()
            .any(|p| p.id == "custom-fixture" && p.connected)
    );
    let config_path = profile
        .path()
        .join("Configurations/providers/opencode.jsonc");
    let config = fs::read_to_string(&config_path).unwrap();
    assert!(config.contains("keep this comment"));
    assert!(!config.contains("fixture-valid-key"));
    assert!(
        !profile
            .path()
            .join("Runtime/providers/data/opencode/auth.json")
            .exists()
    );
    server
        .patch(json!({"model":"custom-fixture/fixture-model","default_agent":"plan"}))
        .unwrap();
    let profile_state=server.patch(json!({"agent":{"reviewer":{"mode":"primary","description":"Local review profile","prompt":"Review carefully."}},"mcp":{"fixture-disabled":{"type":"remote","url":format!("{}/mcp",fixture.url()),"enabled":false}}})).unwrap();
    assert!(profile_state.agents.contains_key("reviewer"));
    assert_eq!(profile_state.mcp["fixture-disabled"]["enabled"], false);
    server.remove_entry("agent", "reviewer").unwrap();
    let removed = server.remove_entry("mcp", "fixture-disabled").unwrap();
    assert!(!removed.custom_agents.contains_key("reviewer"));
    assert!(!removed.mcp.contains_key("fixture-disabled"));
    server
        .test_connection("custom-fixture", "fixture-model")
        .unwrap();
    success.assert();
    server
        .save_provider(
            "custom-fixture",
            "fixture-invalid-key",
            &endpoint,
            "fixture-model",
        )
        .unwrap();
    let error = server
        .test_connection("custom-fixture", "fixture-model")
        .unwrap_err()
        .to_string();
    assert!(!error.contains("do-not-expose"));
    assert!(!error.contains("fixture-invalid-key"));
    rejected.assert();
    let keys = fs::read_dir(profile.path().join("Configurations/providers/keys"))
        .unwrap()
        .count();
    assert_eq!(
        keys, 1,
        "replacing a key removes the old file owned by MyWarp"
    );
    let state = server.disconnect("custom-fixture").unwrap();
    assert!(
        !state
            .providers
            .iter()
            .any(|p| p.id == "custom-fixture" && p.connected)
    );
    assert_eq!(
        fs::read_dir(profile.path().join("Configurations/providers/keys"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
#[ignore = "requires a connected OpenRouter key file and the bundled runtime"]
fn provider_setup_real_openrouter_answers_through_the_native_settings_backend() {
    let executable = std::env::var_os("MYWARP_TEST_OPENCODE").expect("Set MYWARP_TEST_OPENCODE");
    let key_file = std::env::var_os("MYWARP_TEST_OPENROUTER_KEY_FILE")
        .expect("Set MYWARP_TEST_OPENROUTER_KEY_FILE");
    let key = fs::read_to_string(key_file).expect("Read the separately stored provider key");
    let profile = tempfile::tempdir().unwrap();
    let server =
        ProviderServer::start_inner(profile.path().into(), PathBuf::from(executable), false)
            .unwrap();
    let state = server.save_provider("openrouter", &key, "", "").unwrap();
    assert!(
        state
            .providers
            .iter()
            .any(|p| p.id == "openrouter" && p.connected)
    );
    server
        .patch(json!({"model":"openrouter/openai/gpt-4.1-mini"}))
        .unwrap();
    server
        .test_connection("openrouter", "openai/gpt-4.1-mini")
        .unwrap();
}

#[test]
fn provider_edits_update_the_portable_file_without_creating_a_runtime_config() {
    let profile = tempfile::tempdir().unwrap();
    windows_package::prepare_opencode_profile(profile.path()).unwrap();
    let path = configuration_file(profile.path());
    fs::write(&path, "{\n// keep this comment\n\"model\":\"fixture/original\",\"provider\":{\"fixture\":{\"options\":{\"baseURL\":\"http://localhost:1234\",\"apiKey\":\"{file:./keys/test.key}\"}}}}\n").unwrap();

    patch_configuration(
        profile.path(),
        json!({"model":"fixture/updated","provider":{"fixture":{"options":{"baseURL":null}}}}),
    )
    .unwrap();

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("// keep this comment"));
    let saved = configuration(profile.path()).unwrap();
    assert_eq!(saved["model"], "fixture/updated");
    assert_eq!(
        saved["provider"]["fixture"]["options"]["apiKey"],
        "{file:./keys/test.key}"
    );
    assert!(
        saved["provider"]["fixture"]["options"]
            .get("baseURL")
            .is_none()
    );
    assert!(
        !profile
            .path()
            .join("Runtime/providers/config/opencode/opencode.jsonc")
            .exists()
    );
}

#[cfg(feature = "test-util")]
#[test]
#[ignore = "requires the bundled OpenCode executable in MYWARP_TEST_OPENCODE"]
fn copied_provider_configuration_works_with_a_fresh_runtime() {
    let executable =
        PathBuf::from(std::env::var_os("MYWARP_TEST_OPENCODE").expect("Set MYWARP_TEST_OPENCODE"));
    let mut fixture = mockito::Server::new();
    let success = fixture.mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer portable-fixture-key")
        .with_status(200).with_header("content-type", "text/event-stream")
        .with_body(concat!(
            "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"OK\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"fixture\",\"object\":\"chat.completion.chunk\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"))
        .expect_at_least(1).create();
    let source = tempfile::tempdir().unwrap();
    let server =
        ProviderServer::start_inner(source.path().into(), executable.clone(), false).unwrap();
    server
        .save_provider(
            "custom-portable",
            "portable-fixture-key",
            &format!("{}/v1", fixture.url()),
            "fixture-model",
        )
        .unwrap();
    server.patch(json!({"model":"custom-portable/fixture-model", "agent":{"reviewer":{"mode":"primary","prompt":"Review carefully."}}})).unwrap();
    server
        .save_instructions("Use the copied instructions.")
        .unwrap();
    drop(server);
    let source_configuration = windows_package::configuration_dir(source.path());
    let copied = tempfile::tempdir().unwrap();
    let destination = windows_package::configuration_dir(copied.path());
    fs::create_dir_all(destination.join("providers/keys")).unwrap();
    fs::copy(
        source_configuration.join("providers/opencode.jsonc"),
        destination.join("providers/opencode.jsonc"),
    )
    .unwrap();
    fs::copy(
        source_configuration.join("agent-instructions.md"),
        destination.join("agent-instructions.md"),
    )
    .unwrap();
    for entry in fs::read_dir(source_configuration.join("providers/keys")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(
            entry.path(),
            destination.join("providers/keys").join(entry.file_name()),
        )
        .unwrap();
    }
    assert!(!copied.path().join("Runtime").exists());

    let server = ProviderServer::start_inner(copied.path().into(), executable, false).unwrap();
    let loaded = server.snapshot().unwrap();
    assert_eq!(loaded.default_model, "custom-portable/fixture-model");
    assert!(loaded.custom_agents.contains_key("reviewer"));
    server
        .test_connection("custom-portable", "fixture-model")
        .unwrap();
    success.assert();
    let stored = fs::read_to_string(configuration_file(copied.path())).unwrap();
    assert!(stored.contains("{file:./keys/"));
    assert!(stored.contains(windows_package::PERSONAL_INSTRUCTIONS_REFERENCE));
    assert!(!stored.contains(&source.path().to_string_lossy().to_string()));
    assert!(!destination.join("providers/node_modules").exists());
    assert!(!destination.join("providers/package.json").exists());
    assert!(!destination.join("providers/.gitignore").exists());
    assert!(!destination.join("Runtime").exists());
}
