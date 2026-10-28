#[cfg(windows)]
mod windows_check {
    // Manual native rendering check in a disposable portable staging folder.
    use std::path::PathBuf;
    use std::time::Duration;

    use anyhow::{Context as _, Result};
    use pathfinder_geometry::rect::RectF;
    use pathfinder_geometry::vector::vec2f;
    use warp::appearance::AppearanceManager;
    use warp::provider_settings_check::{
        form_is_ready, history_contains_reply, model_is_ready, model_menu_contains,
        model_menu_contains_only, reasoning_is_ready,
    };
    use warp::settings_view::{SEARCH_EDITOR_POSITION_ID, SettingsSection};
    use warp::themes::theme::ThemeKind;
    use warp::workspace::{Workspace, WorkspaceAction};
    use warp_core::AppId;
    use warp_core::channel::{Channel, ChannelConfig, ChannelState, OzConfig, WarpServerConfig};
    use warpui::integration::{AssertionOutcome, Builder, TestStep};
    use warpui::windowing::WindowManager;
    use warpui::{App, SingletonEntity as _, WindowId};

    pub fn run() -> Result<()> {
        let profile = warp_core::windows_package::profile_dir()
            .context("Run the check from a disposable staging folder containing mywarp-portable")?;
        warp_core::windows_package::prepare_opencode_profile(profile)?;
        let profile_text = profile.to_string_lossy();
        let shell_home = profile_text.strip_prefix(r"\\?\").unwrap_or(&profile_text);
        // Set paths before starting any application threads. No real account data is imported.
        unsafe {
            for (name, path) in warp_core::windows_package::opencode_environment(profile) {
                std::env::set_var(name, path);
            }
            std::env::set_var("HOME", shell_home);
            std::env::set_var("USERPROFILE", shell_home);
            std::env::set_var("WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS", "1");
            for (name, _) in std::env::vars_os() {
                let name_text = name.to_string_lossy().to_uppercase();
                if name_text.ends_with("API_KEY")
                    || name_text.ends_with("AUTH_TOKEN")
                    || name_text.starts_with("GOOGLE_")
                {
                    std::env::remove_var(name);
                }
            }
        }
        ChannelState::set(ChannelState::new(
            Channel::Oss,
            ChannelConfig {
                app_id: AppId::new("dev", "goldflyfish", "MyWarpSettingsCheck"),
                logfile_name: "settings-check.log".into(),
                server_config: WarpServerConfig::offline(),
                oz_config: OzConfig {
                    oz_root_url: "http://127.0.0.1:0".into(),
                    workload_audience_url: None,
                },
                telemetry_config: None,
                crash_reporting_config: None,
                autoupdate_config: None,
                mcp_static_config: None,
            },
        ));
        let root = std::env::var_os("MYWARP_UI_CHECK_ROOT")
            .map(PathBuf::from)
            .context("Set MYWARP_UI_CHECK_ROOT to a scratch folder")?;
        if std::env::var_os("MYWARP_UI_CHECK_AUTO_APPROVE").is_some() {
            return auto_approve_check(root);
        }
        if std::env::var_os("MYWARP_UI_CHECK_PROFILE_PARAMETERS").is_some() {
            return profile_parameters_check(root);
        }
        if std::env::var_os("MYWARP_UI_CHECK_REASONING").is_some() {
            return reasoning_check(root);
        }
        let mut builder = Builder::new(root)
            .with_real_display()
            .with_timeout(Duration::from_secs(
                if std::env::var_os("MYWARP_UI_CHECK_WATCH").is_some() {
                    180
                } else {
                    120
                },
            ))
            .with_step(TestStep::new("Open native provider settings").with_action(
                |app, window, _| {
                    if std::env::var_os("MYWARP_UI_CHECK_LIGHT").is_some() {
                        app.update(|ctx| {
                            AppearanceManager::handle(ctx).update(ctx, |manager, ctx| {
                                manager.set_transient_theme(ThemeKind::Light, ctx);
                            })
                        });
                    }
                    app.read(|ctx| {
                        WindowManager::as_ref(ctx).set_window_bounds(
                            window,
                            RectF::new(vec2f(40., 40.), vec2f(1600., 1000.)),
                        )
                    });
                    let workspace = app
                        .views_of_type::<Workspace>(window)
                        .and_then(|v| v.first().cloned())
                        .expect("workspace");
                    app.dispatch_typed_action(
                        window,
                        &[workspace.id()],
                        &WorkspaceAction::ShowSettingsPage(SettingsSection::WarpAgent),
                    );
                },
            ))
            .with_step(
                TestStep::new("Wait for provider directory")
                    .add_assertion(|app, window| {
                        if form_is_ready(app, window) {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure("Provider settings are still loading".into())
                        }
                    })
                    .with_take_screenshot("providers-openrouter.png"),
            )
            .with_step(
                TestStep::new("Select Z.AI Coding Plan")
                    .with_click_on_saved_position("mywarp-provider-action-Z.AI Coding Plan")
                    .set_post_step_pause(Duration::from_millis(200))
                    .with_take_screenshot("providers-zai-coding.png"),
            )
            .with_step(
                TestStep::new("Select Z.AI Standard API")
                    .with_click_on_saved_position("mywarp-provider-action-Z.AI Standard API")
                    .set_post_step_pause(Duration::from_millis(200))
                    .with_take_screenshot("providers-zai-standard.png"),
            )
            .with_step(
                TestStep::new("Select OpenAI and subscription options")
                    .with_click_on_saved_position("mywarp-provider-action-OpenAI / Codex")
                    .set_post_step_pause(Duration::from_millis(200))
                    .with_take_screenshot("providers-openai.png"),
            );
        if let Ok(expected) = std::env::var("MYWARP_UI_CHECK_VERIFICATION_RESTORE") {
            builder = builder
                .with_step(
                    TestStep::new("Select the saved provider after restarting")
                        .with_click_on_saved_position("mywarp-provider-action-Custom endpoint"),
                )
                .with_step(
                    provider_marker_step("Restore saved verification", &expected)
                        .with_take_screenshot("provider-verification-restored.png"),
                );
            return warp::run_integration_test(
                builder.build("mywarp_provider_verification_restore", false),
            );
        }
        if std::env::var_os("MYWARP_UI_CHECK_LAYOUT_ONLY").is_some() {
            builder = builder
                .with_step(search("verification"))
                .with_step(
                    TestStep::new("Capture the provider search result")
                        .with_take_screenshot("providers-filtered.png"),
                )
                .with_step(search(""))
                .with_step(
                    TestStep::new("Capture the restored settings")
                        .with_take_screenshot("providers-restored.png"),
                )
                .with_step(
                    TestStep::new("Open the separate Warp Agent behavior section")
                        .with_click_on_saved_position(warp::settings_view::nav_subpage_position_id(
                            SettingsSection::WarpAgentBehavior,
                        ))
                        .with_take_screenshot("warp-agent-behavior.png"),
                )
                .with_step(search("input hint"))
                .with_step(
                    TestStep::new(
                        "Behavior search keeps the title and excludes provider configuration",
                    )
                    .add_assertion(|app, window| {
                        let present = |id| {
                            app.read(|ctx| ctx.element_position_by_id_at_last_frame(window, id))
                                .is_some()
                        };
                        let behavior_active = app
                            .views_of_type::<warp::settings_view::SettingsView>(window)
                            .is_some_and(|views| {
                                views.iter().any(|view| {
                                    view.read(app, |view, _| {
                                        view.current_settings_section()
                                            == SettingsSection::WarpAgentBehavior
                                    })
                                })
                            });
                        if behavior_active
                            && present("warp-agent-enabled")
                            && present("warp-agent-show-input-hint")
                        {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure(
                                "Behavior search did not isolate the original control".into(),
                            )
                        }
                    })
                    .with_take_screenshot("warp-agent-behavior-filtered.png"),
                )
                .with_step(
                    TestStep::new("Disable the agent with the original master switch")
                        .with_click_on_saved_position("warp-agent-enabled")
                        .add_assertion(|app, _| {
                            if !app.read(|ctx| {
                                warp::settings::AISettings::as_ref(ctx).is_any_ai_enabled(ctx)
                            }) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "The master switch did not disable the native agent".into(),
                                )
                            }
                        }),
                )
                .with_step(
                    TestStep::new("Re-enable the agent with the original master switch")
                        .with_click_on_saved_position("warp-agent-enabled")
                        .add_assertion(|app, _| {
                            if app.read(|ctx| {
                                warp::settings::AISettings::as_ref(ctx).is_any_ai_enabled(ctx)
                            }) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "The master switch did not enable the native agent".into(),
                                )
                            }
                        }),
                )
                .with_step(search("voice"))
                .with_step(
                    TestStep::new("Capture voice availability")
                        .with_take_screenshot("warp-agent-voice.png"),
                )
                .with_step(search(""))
                .with_step(
                    TestStep::new("Capture the restored behavior page")
                        .with_take_screenshot("warp-agent-restored.png"),
                );
            builder = builder
                .with_step(search("next command"))
                .with_step(
                    TestStep::new("Disable Active AI")
                        .with_click_on_saved_position("warp-active-ai-enabled")
                        .add_assertion(|app, _| {
                            if !app.read(|ctx| {
                                warp::settings::AISettings::as_ref(ctx).is_active_ai_enabled(ctx)
                            }) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Active AI did not disable".into())
                            }
                        }),
                )
                .with_step(
                    TestStep::new("Enable Active AI")
                        .with_click_on_saved_position("warp-active-ai-enabled")
                        .add_assertion(|app, _| {
                            if app.read(|ctx| {
                                warp::settings::AISettings::as_ref(ctx).is_active_ai_enabled(ctx)
                            }) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Active AI did not enable".into())
                            }
                        }),
                );
            type Enabled = fn(&warp::settings::AISettings, &warpui::AppContext) -> bool;
            let toggles: [(&str, &str, Enabled); 6] = [
                (
                    "next command",
                    "IntelligentAutosuggestionsEnabled",
                    warp::settings::AISettings::is_intelligent_autosuggestions_enabled,
                ),
                (
                    "prompt suggestions",
                    "AgentModeQuerySuggestionsEnabled",
                    warp::settings::AISettings::is_prompt_suggestions_enabled,
                ),
                (
                    "code banners",
                    "CodeSuggestionsEnabled",
                    warp::settings::AISettings::is_code_suggestions_enabled,
                ),
                (
                    "shared block title",
                    "SharedBlockTitleGenerationEnabled",
                    warp::settings::AISettings::is_shared_block_title_generation_enabled,
                ),
                (
                    "commit pull",
                    "GitOperationsAutogenEnabled",
                    warp::settings::AISettings::is_git_operations_autogen_enabled,
                ),
                (
                    "voice",
                    "VoiceInputEnabled",
                    warp::settings::AISettings::is_voice_input_enabled,
                ),
            ];
            for (query, key, enabled) in toggles {
                builder = builder
                    .with_step(search(query))
                    .with_step(
                        TestStep::new(&format!("Disable {key}"))
                            .with_click_on_saved_position(format!("ai-setting-toggle-{key}"))
                            .add_assertion(move |app, _| {
                                if !app.read(|ctx| {
                                    enabled(warp::settings::AISettings::as_ref(ctx), ctx)
                                }) {
                                    AssertionOutcome::Success
                                } else {
                                    AssertionOutcome::failure(format!("{key} did not disable"))
                                }
                            }),
                    )
                    .with_step(
                        TestStep::new(&format!("Enable {key}"))
                            .with_click_on_saved_position(format!("ai-setting-toggle-{key}"))
                            .add_assertion(move |app, _| {
                                if app.read(|ctx| {
                                    enabled(warp::settings::AISettings::as_ref(ctx), ctx)
                                }) {
                                    AssertionOutcome::Success
                                } else {
                                    AssertionOutcome::failure(format!("{key} did not enable"))
                                }
                            }),
                    );
            }
            builder = builder.with_step(search("")).with_step(
                TestStep::new("Capture enabled behavior settings")
                    .with_take_screenshot("warp-agent-enabled.png"),
            );
            return warp::run_integration_test(builder.build("mywarp_provider_settings", false));
        }
        let expect_error = std::env::var_os("MYWARP_UI_CHECK_ERROR").is_some();
        let cancel_test = std::env::var_os("MYWARP_UI_CHECK_CANCEL").is_some();
        if let Some(endpoint) = std::env::var_os("MYWARP_UI_CHECK_ENDPOINT") {
            builder = builder
                .with_step(
                    TestStep::new("Choose a custom provider").with_click_on_saved_position(
                        if std::env::var_os("MYWARP_UI_CHECK_SAVE_RACE").is_some() {
                            "mywarp-provider-action-Custom endpoint"
                        } else {
                            "mywarp-provider-picker"
                        },
                    ),
                )
                .with_step(if std::env::var_os("MYWARP_UI_CHECK_SAVE_RACE").is_some() {
                    TestStep::new("Use the fixture provider")
                } else {
                    TestStep::new("Filter provider directory")
                        .with_typed_characters(&["Custom OpenAI-compatible"])
                        .with_keystrokes(&["down", "enter"])
                })
                .with_step(
                    TestStep::new("Wait for custom provider fields").add_assertion(
                        |app, window| {
                            if app
                                .read(|ctx| {
                                    ctx.element_position_by_id_at_last_frame(
                                        window,
                                        "mywarp-provider-field-Custom model ID",
                                    )
                                })
                                .is_some()
                            {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "Custom provider fields are not visible".into(),
                                )
                            }
                        },
                    ),
                )
                .with_step(fill("API key", "ui-check-key"))
                .with_step(fill("Base URL", &endpoint.to_string_lossy()))
                .with_step(fill("Custom model ID", "ui-fixture"))
                .with_step(if std::env::var_os("MYWARP_UI_CHECK_SAVE_RACE").is_some() {
                    TestStep::new("Save connection while its configuration reloads").with_action(
                        |app, window, _| {
                            warp::settings_view::native_save_provider_with_configuration_reload(
                                app, window,
                            );
                        },
                    )
                } else {
                    TestStep::new("Save connection in the native form")
                        .with_click_on_saved_position("mywarp-provider-action-Save connection-Save")
                })
                .with_step(
                    TestStep::new("Verify saved configuration")
                        .add_assertion(move |app, window| {
                            let config = std::fs::read_to_string(
                                profile.join("Configurations/providers/opencode.jsonc"),
                            )
                            .unwrap_or_default();
                            if form_is_ready(app, window)
                                && warp::provider_settings_check::provider_key_is_masked(
                                    app, window,
                                )
                                && config.contains("ui-fixture")
                                && config.contains("{file:./keys/mywarp-custom-mywarp-")
                                && !config.contains("ui-check-key")
                            {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "The native form has not saved the connection".into(),
                                )
                            }
                        })
                        .with_take_screenshot("providers-custom-saved.png"),
                )
                .with_step(
                    TestStep::new("Open provider configuration in the built-in editor")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Open provider config-OpenConfiguration",
                        )
                        .add_assertion(|app, window| {
                            if warp::provider_settings_check::configuration_is_open(
                                app, window, false,
                            ) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "Provider configuration was not opened".into(),
                                )
                            }
                        })
                        .with_take_screenshot("provider-config-editor.png"),
                )
                .with_step(TestStep::new("Return to provider settings").with_action(
                    |app, window, _| {
                        workspace_action(
                            app,
                            window,
                            WorkspaceAction::ShowSettingsPage(SettingsSection::WarpAgent),
                        )
                    },
                ))
                .with_step(
                    TestStep::new("Open the saved API key file")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Open key file-OpenApiKeyFile",
                        )
                        .add_assertion(|app, window| {
                            if warp::provider_settings_check::configuration_is_open(
                                app, window, true,
                            ) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Saved key file was not opened".into())
                            }
                        }),
                )
                .with_step(
                    TestStep::new("Return from the key file to settings").with_action(
                        |app, window, _| {
                            workspace_action(
                                app,
                                window,
                                WorkspaceAction::ShowSettingsPage(SettingsSection::WarpAgent),
                            )
                        },
                    ),
                )
                .with_step(search("model"))
                .with_step(
                    TestStep::new("Wait for the provider form to finish reloading").add_assertion(
                        |app, window| {
                            if form_is_ready(app, window) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Provider form is still reloading".into())
                            }
                        },
                    ),
                )
                .with_step(
                    TestStep::new("Test the provider through the native button")
                        .with_click_on_saved_position("mywarp-provider-action-Test connection-Test")
                        .set_post_step_pause(if cancel_test {
                            Duration::from_secs(2)
                        } else {
                            Duration::from_millis(500)
                        })
                        .with_take_screenshot("provider-connection-testing.png"),
                );
            if cancel_test {
                builder = builder.with_step(
                    TestStep::new("Cancel the pending connection test")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Cancel test-CancelTest",
                        ),
                );
            }
            builder = builder.with_step(
                TestStep::new("Verify the model answered")
                    .set_timeout(Duration::from_secs(60))
                    .add_assertion(move |app, window| {
                        if app
                            .read(|ctx| {
                                ctx.element_position_by_id_at_last_frame(
                                    window,
                                    if cancel_test {
                                        "mywarp-connection-cancelled"
                                    } else if expect_error {
                                        "mywarp-connection-error"
                                    } else {
                                        "mywarp-connection-verified"
                                    },
                                )
                            })
                            .is_some()
                        {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure(
                                "The native connection test has not succeeded".into(),
                            )
                        }
                    })
                    .with_take_screenshot("provider-connection-result.png"),
            );
        }
        if std::env::var_os("MYWARP_UI_CHECK_VERIFICATION").is_some() {
            builder = builder
                .with_step(
                    provider_marker_step("Verified providers have yellow markers", "Verified")
                        .with_take_screenshot("provider-marker-yellow.png"),
                )
                .with_step(
                    TestStep::new("Choose the verified provider as default")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Set as default-DefaultModel",
                        ),
                )
                .with_step(
                    provider_marker_step("The default provider has a green marker", "Default")
                        .with_take_screenshot("provider-marker-green.png"),
                )
                .with_step(
                    TestStep::new("Refresh the saved provider directory")
                        .with_click_on_saved_position("mywarp-provider-action-Refresh-Refresh"),
                )
                .with_step(
                    provider_marker_step(
                        "Refresh preserves the default and verification",
                        "Default",
                    )
                    .with_take_screenshot("provider-marker-refreshed.png"),
                );
            return warp::run_integration_test(
                builder.build("mywarp_provider_verification", false),
            );
        }
        if expect_error || cancel_test || std::env::var_os("MYWARP_UI_CHECK_SAVE_RACE").is_some() {
            return warp::run_integration_test(builder.build("mywarp_provider_settings", false));
        }
        let mut builder = builder
            .with_step(search("profile"))
            .with_step(
                TestStep::new("Create a custom agent profile").with_click_on_saved_position(
                    "mywarp-provider-action-New agent profile-ShowProfileEditor(true)",
                ),
            )
            .with_step(fill("Profile name", "ui-review"))
            .with_step(fill(
                "Role and instructions",
                "UI_REVIEW_PROFILE: Answer clearly. Do not use tools in this check.",
            ))
            .with_step(
                TestStep::new("Save the custom profile").with_click_on_saved_position(
                    "mywarp-provider-action-Save profile-SaveProfile",
                ),
            )
            .with_step(
                TestStep::new("Wait for the profile to be saved").add_assertion(
                    move |app, window| {
                        let config = std::fs::read_to_string(
                            profile.join("Configurations/providers/opencode.jsonc"),
                        )
                        .unwrap_or_default();
                        if form_is_ready(app, window) && config.contains("UI_REVIEW_PROFILE") {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure("The profile is not saved yet".into())
                        }
                    },
                ),
            )
            .with_step(
                TestStep::new("Open the native profile picker")
                    .with_click_on_saved_position("mywarp-agent-profile-picker")
                    .with_take_screenshot("agent-profile-menu.png"),
            )
            .with_step(
                TestStep::new("Select the custom profile")
                    .with_keystrokes(&["down", "down", "enter"]),
            )
            .with_step(
                TestStep::new("Wait for the custom default profile")
                    .add_assertion(move |app, window| {
                        let preferences = std::fs::read_to_string(
                            profile.join("Configurations/agent-preferences.json"),
                        )
                        .unwrap_or_default();
                        if form_is_ready(app, window) && preferences.contains("ui-review") {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure(
                                "The custom profile is not selected yet".into(),
                            )
                        }
                    })
                    .with_take_screenshot("agent-profiles.png"),
            )
            .with_step(search("permissions"))
            .with_step(
                TestStep::new("Capture tool permissions")
                    .with_take_screenshot("agent-permissions.png"),
            )
            .with_step(search("personal instructions"))
            .with_step(
                TestStep::new("Capture personal instructions")
                    .with_take_screenshot("agent-instructions.png"),
            )
            .with_step(search("MCP"))
            .with_step(
                TestStep::new("Capture MCP configuration").with_take_screenshot("agent-mcp.png"),
            )
            .with_step(
                TestStep::new("Choose local MCP command setup")
                    .with_click_on_saved_position("mywarp-mcp-type-picker"),
            )
            .with_step(
                TestStep::new("Show local command arguments")
                    .with_keystrokes(&["down", "enter"])
                    .with_take_screenshot("agent-mcp-local.png"),
            );
        if std::env::var_os("MYWARP_UI_CHECK_ENDPOINT").is_some() {
            builder = builder
                .with_step(search("model"))
                .with_step(
                    TestStep::new("Wait for the default model action to be available")
                        .add_assertion(|app, window| {
                            if form_is_ready(app, window) {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Provider form is still reloading".into())
                            }
                        }),
                )
                .with_step(
                    TestStep::new("Set default model through Settings")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Set as default-DefaultModel",
                        ),
                )
                .with_step(
                    TestStep::new("Wait for default model to be saved")
                        .add_assertion(move |app, window| {
                            let config = std::fs::read_to_string(
                                profile.join("Configurations/providers/opencode.jsonc"),
                            )
                            .unwrap_or_default();
                            let saved_model = serde_json::from_str::<serde_json::Value>(&config)
                                .ok()
                                .and_then(|config| config["model"].as_str().map(str::to_owned));
                            if form_is_ready(app, window)
                                && saved_model.as_deref() == Some("custom-mywarp/ui-fixture")
                            {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Default model not yet saved".into())
                            }
                        })
                        .with_take_screenshot("providers-default-model.png"),
                )
                .with_step(
                    TestStep::new("Start provider-backed background feature checks").with_action(
                        |app, window, _| {
                            if std::env::var_os("MYWARP_UI_CHECK_AUXILIARY").is_some() {
                                warp::provider_settings_check::start_auxiliary_check(app, window);
                            }
                        },
                    ),
                )
                .with_step(
                    TestStep::new("Verify provider-backed background features")
                        .set_timeout(Duration::from_secs(90))
                        .add_assertion(|app, _| {
                            if std::env::var_os("MYWARP_UI_CHECK_AUXILIARY").is_none() {
                                return AssertionOutcome::Success;
                            }
                            match warp::provider_settings_check::auxiliary_check_result(app) {
                                Some(Ok(())) => AssertionOutcome::Success,
                                Some(Err(error)) => AssertionOutcome::failure(error),
                                None => AssertionOutcome::failure(
                                    "Provider-backed feature checks are still running".into(),
                                ),
                            }
                        }),
                )
                .with_step(
                    TestStep::new("Open native agent tab").with_action(|app, window, _| {
                        workspace_action(app, window, WorkspaceAction::AddAgentTab)
                    }),
                )
                .with_step(
                    TestStep::new("Wait for native chat to load the configured model")
                        .set_timeout(Duration::from_secs(60))
                        .add_assertion(|app, window| {
                            let configured =
                                model_is_ready(app, window, "custom-mywarp/ui-fixture");
                            if configured {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Native ACP model is not ready".into())
                            }
                        }),
                )
                .with_step(TestStep::new("Enter a native agent prompt").with_action(
                    |app, window, _| {
                        let workspace = app
                            .views_of_type::<Workspace>(window)
                            .and_then(|v| v.first().cloned())
                            .expect("workspace");
                        workspace.update(app, |workspace, ctx| {
                            workspace.set_active_terminal_input_contents_and_focus_app(
                                "Say hello from the configured provider.",
                                ctx,
                            )
                        });
                    },
                ))
                .with_step(
                    TestStep::new("Submit through the agent input").with_keystrokes(&["enter"]),
                )
                .with_step(
                    TestStep::new("Verify the response reached native conversation history")
                        .set_timeout(Duration::from_secs(60))
                        .add_assertion(|app, window| {
                            let answered =
                                history_contains_reply(app, window, "MYWARP_CONNECTION_OK");
                            if answered {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "Native conversation has not received the model response"
                                        .into(),
                                )
                            }
                        })
                        .with_take_screenshot("native-provider-chat.png"),
                )
                .with_step(
                    TestStep::new("Open provider settings").with_action(|app, window, _| {
                        workspace_action(
                            app,
                            window,
                            WorkspaceAction::ShowSettingsPageWithSearch {
                                search_query: String::new(),
                                section: Some(SettingsSection::WarpAgent),
                            },
                        );
                    }),
                )
                .with_step(
                    TestStep::new("Verify provider navigation clears search")
                        .add_assertion(|app, window| {
                            if form_is_ready(app, window)
                                && app
                                    .read(|ctx| {
                                        ctx.element_position_by_id_at_last_frame(
                                            window,
                                            "mywarp-provider-picker",
                                        )
                                    })
                                    .is_some()
                            {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure(
                                    "Provider configuration is not visible yet".into(),
                                )
                            }
                        })
                        .with_take_screenshot("providers-final.png"),
                );
        }
        if std::env::var_os("MYWARP_UI_CHECK_WATCH").is_some() {
            builder = builder
                .with_step(fill("API key","unsaved-key"))
                .with_step(TestStep::new("Edit provider models on disk without Refresh").with_action(move |_,_,_| {
                    let path = warp_core::provider_setup::configuration_file(profile);
                    let mut config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                    config["provider"]["custom-mywarp"]["models"]["ui-second"] = serde_json::json!({"name":"Second fixture"});
                    config["model"] = "custom-mywarp/ui-second".into();
                    std::fs::write(path,serde_json::to_vec_pretty(&config).unwrap()).unwrap();
                }))
                .with_step(TestStep::new("Reload settings and existing chat while preserving the unsaved key")
                    .set_timeout(Duration::from_secs(25))
                    .add_assertion(|app,window| {
                        if warp::provider_settings_check::configuration_matches(app,window,"ui-second","unsaved-key",false)
                            && warp::provider_settings_check::chat_configuration_has_model(app,"custom-mywarp/ui-second")
                        { AssertionOutcome::Success } else { AssertionOutcome::failure("External model edit has not reached settings and the live chat".into()) }
                    }).with_take_screenshot("configuration-reloaded-draft-preserved.png"))
                .with_step(TestStep::new("Save an incomplete configuration edit").with_action(move |_,_,_| {
                    let path = warp_core::provider_setup::configuration_file(profile);
                    std::fs::copy(&path,profile.join("watch-valid-config.json")).unwrap();
                    std::fs::write(path,"{\"provider\":").unwrap();
                }))
                .with_step(TestStep::new("Report invalid JSONC and retain the current chat")
                    .set_timeout(Duration::from_secs(20))
                    .add_assertion(|app,window| {
                        if warp::provider_settings_check::configuration_matches(app,window,"ui-second","unsaved-key",true)
                            && warp::provider_settings_check::chat_configuration_has_model(app,"custom-mywarp/ui-second")
                        { AssertionOutcome::Success } else { AssertionOutcome::failure("The invalid edit was not reported or the current configuration was discarded".into()) }
                    }).with_take_screenshot("configuration-invalid-kept-active.png"))
                .with_step(TestStep::new("Repair the configuration and rotate its referenced key").with_action(move |_,_,_| {
                    let path = warp_core::provider_setup::configuration_file(profile);
                    let mut config: serde_json::Value = serde_json::from_slice(&std::fs::read(profile.join("watch-valid-config.json")).unwrap()).unwrap();
                    config["provider"]["custom-mywarp"]["models"]["ui-third"] = serde_json::json!({"name":"Third fixture"});
                    let reference = config["provider"]["custom-mywarp"]["options"]["apiKey"].as_str().unwrap();
                    let key = reference.strip_prefix("{file:").unwrap().strip_suffix('}').unwrap();
                    std::fs::write(path.parent().unwrap().join(key),"ui-rotated-key").unwrap();
                    std::fs::write(path,serde_json::to_vec_pretty(&config).unwrap()).unwrap();
                }))
                .with_step(TestStep::new("Recover automatically after the corrected edit")
                    .set_timeout(Duration::from_secs(25))
                    .add_assertion(|app,window| {
                        if warp::provider_settings_check::configuration_matches(app,window,"ui-third","unsaved-key",false)
                            && warp::provider_settings_check::chat_configuration_has_model(app,"custom-mywarp/ui-third")
                        { AssertionOutcome::Success } else { AssertionOutcome::failure("The corrected configuration has not reloaded".into()) }
                    }).with_take_screenshot("configuration-recovered.png"))
                .with_step(fill("API key",""))
                .with_step(TestStep::new("Return to the existing native chat").with_action(|app,window,_|workspace_action(app,window,WorkspaceAction::ActivateLastTab)))
                .with_step(TestStep::new("The existing conversation and selected model survive reload").add_assertion(|app,window| {
                    if model_is_ready(app,window,"custom-mywarp/ui-fixture") && history_contains_reply(app,window,"MYWARP_CONNECTION_OK")
                    { AssertionOutcome::Success } else { AssertionOutcome::failure("The reloaded chat lost its selected model or history".into()) }
                }))
                .with_step(TestStep::new("Prompt the existing chat with the new credentials").with_action(|app,window,_| {
                    let workspace = app.views_of_type::<Workspace>(window).and_then(|views|views.first().cloned()).expect("workspace");
                    workspace.update(app,|workspace,ctx|workspace.set_active_terminal_input_contents_and_focus_app("Reply using the rotated fixture key.",ctx));
                }))
                .with_step(TestStep::new("Submit after automatic key reload").with_keystrokes(&["enter"]))
                .with_step(TestStep::new("The live chat uses the rotated key without Reconnect")
                    .set_timeout(Duration::from_secs(40))
                    .add_assertion(|app,window| {
                        if history_contains_reply(app,window,"MYWARP_ROTATED_OK") { AssertionOutcome::Success }
                        else { AssertionOutcome::failure("The rotated key did not reach the native response".into()) }
                    }).with_take_screenshot("native-chat-rotated-key.png"));
        }
        let driver = builder.build("mywarp_provider_settings", false);
        warp::run_integration_test(driver)
    }

    fn auto_approve_check(root: PathBuf) -> Result<()> {
        let mut builder = Builder::new(root)
            .with_real_display()
            .with_timeout(Duration::from_secs(90))
            .with_step(
                TestStep::new("Open native chat").with_action(|app, window, _| {
                    workspace_action(app, window, WorkspaceAction::AddAgentTab);
                }),
            )
            .with_step(
                TestStep::new("Wait for the provider session").add_assertion(|app, window| {
                    if warp::provider_settings_check::chat_is_ready(app, window) {
                        AssertionOutcome::Success
                    } else {
                        AssertionOutcome::failure("Provider session is not ready".into())
                    }
                }),
            );
        if std::env::var_os("MYWARP_UI_CHECK_AUTO_APPROVE_RESTORED").is_none() {
            builder = builder
                .with_step(
                    TestStep::new("Open the persistent approval choices")
                        .with_click_on_saved_position("mywarp-chat-auto-approve"),
                )
                .with_step(
                    TestStep::new("Enable auto-approve for the active profile")
                        .with_keystrokes(&["down", "enter"]),
                );
        }
        builder = builder
            .with_step(
                TestStep::new("Confirm the profile keeps auto-approve enabled")
                    .add_assertion(|app, window| {
                        if warp::provider_settings_check::auto_approve_is_enabled(app, window) {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure(
                                "Auto-approve is not enabled for the active profile".into(),
                            )
                        }
                    })
                    .with_take_screenshot("auto-approve-mode.png"),
            )
            .with_step(
                TestStep::new("Submit through the native chat controller").with_action(
                    |app, window, _| {
                        // Windows integration sessions do not bootstrap a real shell.
                        warp::provider_settings_check::submit_auto_approve_fixture(app, window);
                    },
                ),
            )
            .with_step(
                TestStep::new("Complete reads and file changes without approval clicks")
                    .set_timeout(Duration::from_secs(60))
                    .add_assertion(|app, window| {
                        if history_contains_reply(app, window, "AUTO_APPROVE_POLICY_OK") {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure(
                                "The commands and file changes have not completed".into(),
                            )
                        }
                    })
                    .with_take_screenshot("auto-approve-completed.png"),
            );
        warp::run_integration_test(builder.build("mywarp_auto_approve", false))
    }

    fn profile_parameters_check(root: PathBuf) -> Result<()> {
        let builder = Builder::new(root)
            .with_real_display()
            .with_step(
                TestStep::new("Open profile parameters").with_action(|app, window, _| {
                    warp::provider_settings_check::create_execution_profile_fixture(app);
                    app.read(|ctx| {
                        WindowManager::as_ref(ctx).set_window_bounds(
                            window,
                            RectF::new(vec2f(40., 40.), vec2f(1600., 1100.)),
                        )
                    });
                    workspace_action(
                        app,
                        window,
                        WorkspaceAction::ShowSettingsPage(SettingsSection::AgentProfiles),
                    );
                }),
            )
            .with_step(
                TestStep::new("Show all profile models and permissions")
                    .add_assertion(|app, window| {
                        if warp::provider_settings_check::profiles_are_ready(app, window) {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure("Profiles are still loading".into())
                        }
                    })
                    .with_take_screenshot("profile-parameters.png"),
            )
            .with_step(search("mcp denylist"))
            .with_step(
                TestStep::new("Find restored profile permissions through search")
                    .add_assertion(|app, window| {
                        if warp::provider_settings_check::profiles_are_ready(app, window) {
                            AssertionOutcome::Success
                        } else {
                            AssertionOutcome::failure("Profile permissions did not match".into())
                        }
                    })
                    .with_take_screenshot("profile-parameters-search.png"),
            )
            .with_step(search(""))
            .with_step(
                TestStep::new("Show restored profiles after clearing search")
                    .with_take_screenshot("profile-parameters-restored.png"),
            )
            .with_step(
                TestStep::new("Open the complete profile editor").with_action(|app, window, _| {
                    app.read(|ctx| {
                        WindowManager::as_ref(ctx).set_window_bounds(
                            window,
                            RectF::new(vec2f(40., 40.), vec2f(1600., 2600.)),
                        )
                    });
                    warp::provider_settings_check::open_default_execution_profile_editor(
                        app, window,
                    );
                }),
            )
            .with_step(
                TestStep::new("Capture all editable profile parameters")
                    .with_take_screenshot("profile-parameters-editor.png"),
            );
        warp::run_integration_test(builder.build("mywarp_profile_parameters", false))
    }

    fn reasoning_check(root: PathBuf) -> Result<()> {
        let builder = Builder::new(root).with_real_display().with_timeout(Duration::from_secs(90))
            .with_step(TestStep::new("Save an original execution profile with a provider model").with_action(|app,_,_| {
                warp::provider_settings_check::create_execution_profile_fixture(app);
            }))
            .with_step(TestStep::new("Open a chat with the configured model").with_action(|app,window,_| {
                workspace_action(app,window,WorkspaceAction::AddAgentTab);
            }))
            .with_step(TestStep::new("Show the selected model and supported reasoning effort")
                .add_assertion(|app,window| {
                    if model_is_ready(app,window,"custom-mywarp/fixture")
                        && model_menu_contains_only(app,window,"custom-mywarp/fixture")
                        && reasoning_is_ready(app,window,"low") {
                        AssertionOutcome::Success
                    } else { AssertionOutcome::failure("The configured model and low reasoning effort are not ready".into()) }
                }).with_take_screenshot("reasoning-low.png"))
            .with_step(TestStep::new("Open reasoning effort choices").with_click_on_saved_position("mywarp-chat-reasoning-effort").with_take_screenshot("reasoning-choices.png"))
            .with_step(TestStep::new("Choose high reasoning effort").with_keystrokes(&["down","down","enter"]))
            .with_step(TestStep::new("Confirm high reasoning effort is applied and saved")
                .add_assertion(|app,window| {
                    if reasoning_is_ready(app,window,"high") { AssertionOutcome::Success }
                    else { AssertionOutcome::failure("High reasoning effort has not been confirmed and saved".into()) }
                }).with_take_screenshot("reasoning-high.png"))
            .with_step(TestStep::new("Reload the conversation after a configuration change").with_action(|_, _, _| {
                let profile = warp_core::windows_package::profile_dir().expect("portable profile");
                let path = warp_core::provider_setup::configuration_file(profile);
                let mut config: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                config["provider"]["custom-mywarp"]["models"]["reasoning-reload"] = serde_json::json!({"name":"Reasoning reload fixture"});
                std::fs::write(path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
            }))
            .with_step(TestStep::new("Restore the saved reasoning effort after automatic reload")
                .add_assertion(|app,window| {
                    if reasoning_is_ready(app,window,"high")
                        && warp::provider_settings_check::chat_configuration_has_model(app,"custom-mywarp/reasoning-reload") { AssertionOutcome::Success }
                    else { AssertionOutcome::failure("The reconnected conversation has not restored high reasoning effort".into()) }
                }).with_take_screenshot("reasoning-reconnected.png"))
            .with_step(TestStep::new("Open the restored Profiles page").with_action(|app, window, _| {
                workspace_action(app, window, WorkspaceAction::ShowSettingsPageWithSearch {
                    search_query: String::new(),
                    section: Some(SettingsSection::AgentProfiles),
                });
            }))
            .with_step(TestStep::new("Show original profiles with their provider models")
                .add_assertion(|app,window| {
                    if warp::provider_settings_check::profiles_are_ready(app,window) { AssertionOutcome::Success }
                    else { AssertionOutcome::failure("The original Profiles page and saved profile are unavailable".into()) }
                }).with_take_screenshot("original-profiles.png"))
            .with_step(search("profiles models"))
            .with_step(TestStep::new("Keep Profiles visible during a model search")
                .add_assertion(|app,window| {
                    if warp::provider_settings_check::profiles_are_ready(app,window) { AssertionOutcome::Success }
                    else { AssertionOutcome::failure("Profiles disappeared during model search".into()) }
                }).with_take_screenshot("profiles-model-search.png"))
            .with_step(search(""))
            .with_step(TestStep::new("Return to chat").with_action(|app,window,_| {
                workspace_action(app,window,WorkspaceAction::CloseActiveTab);
            }))
            .with_step(TestStep::new("Open execution profile choices").with_click_on_saved_position("mywarp-chat-profile").with_take_screenshot("profile-choices.png"))
            .with_step(TestStep::new("Select the saved execution profile").with_keystrokes(&["down","enter"]))
            .with_step(TestStep::new("Apply the saved profile model to the conversation")
                .add_assertion(|app,window| {
                    if model_is_ready(app,window,"custom-mywarp/hidden") && model_menu_contains(app,window,"custom-mywarp/hidden") { AssertionOutcome::Success }
                    else { AssertionOutcome::failure("The saved profile's provider model has not been applied".into()) }
                }).with_take_screenshot("profile-model-applied.png"));
        warp::run_integration_test(builder.build("mywarp_reasoning_effort", false))
    }

    fn provider_marker_step(name: &str, expected: &str) -> TestStep {
        let position = format!("mywarp-provider-marker-custom-mywarp-{expected}");
        TestStep::new(name).add_assertion(move |app, window| {
            if form_is_ready(app, window)
                && app
                    .read(|ctx| ctx.element_position_by_id_at_last_frame(window, &position))
                    .is_some()
            {
                AssertionOutcome::Success
            } else {
                AssertionOutcome::failure(format!("Provider marker has not become {position}"))
            }
        })
    }

    fn workspace_action(app: &mut App, window: WindowId, action: WorkspaceAction) {
        let workspace = app
            .views_of_type::<Workspace>(window)
            .and_then(|v| v.first().cloned())
            .expect("workspace");
        app.dispatch_typed_action(window, &[workspace.id()], &action);
    }

    fn search(query: &'static str) -> TestStep {
        TestStep::new(&format!("Search for {query}"))
            .with_click_on_saved_position(SEARCH_EDITOR_POSITION_ID)
            .with_keystrokes(&["cmdorctrl-a", "backspace"])
            .with_typed_characters(&[query])
    }

    fn fill(label: &str, text: &str) -> TestStep {
        TestStep::new(&format!("Fill {label}"))
            .with_click_on_saved_position(format!("mywarp-provider-field-{label}"))
            .with_keystrokes(&["cmdorctrl-a", "backspace"])
            .with_typed_characters(&[text])
    }
}
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    windows_check::run()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This portable-profile rendering check runs on Windows.");
}
