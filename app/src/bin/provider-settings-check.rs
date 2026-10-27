#[cfg(windows)]
mod windows_check {
    // Manual native rendering check in a disposable portable staging folder.
    use std::path::PathBuf;
    use std::time::Duration;

    use anyhow::{Context as _, Result};
    use pathfinder_geometry::rect::RectF;
    use pathfinder_geometry::vector::vec2f;
    use warp::provider_settings_check::{form_is_ready, history_contains_reply, model_is_ready};
    use warp::settings_view::{SEARCH_EDITOR_POSITION_ID, SettingsSection};
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
        let mut builder = Builder::new(root)
            .with_real_display()
            .with_timeout(Duration::from_secs(120))
            .with_step(TestStep::new("Open native provider settings").with_action(
                |app, window, _| {
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
                TestStep::new("Select OpenAI and subscription options")
                    .with_click_on_saved_position("mywarp-provider-action-OpenAI / Codex")
                    .set_post_step_pause(Duration::from_millis(200))
                    .with_take_screenshot("providers-openai.png"),
            );
        if let Some(endpoint) = std::env::var_os("MYWARP_UI_CHECK_ENDPOINT") {
            builder = builder
                .with_step(
                    TestStep::new("Choose a custom provider")
                        .with_click_on_saved_position("mywarp-provider-picker"),
                )
                .with_step(
                    TestStep::new("Filter provider directory")
                        .with_typed_characters(&["Custom OpenAI-compatible"])
                        .with_keystrokes(&["enter"]),
                )
                .with_step(fill("API key", "ui-check-key"))
                .with_step(fill(
                    "Base URL  ·  optional for built-in providers",
                    &endpoint.to_string_lossy(),
                ))
                .with_step(fill("Custom model ID", "ui-fixture"))
                .with_step(
                    TestStep::new("Save connection in the native form")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Save connection-Save",
                        ),
                )
                .with_step(
                    TestStep::new("Verify saved configuration")
                        .add_assertion(move |app, window| {
                            let config = std::fs::read_to_string(
                                profile.join("providers/config/opencode/opencode.jsonc"),
                            )
                            .unwrap_or_default();
                            if form_is_ready(app, window)
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
                .with_step(search("model"))
                .with_step(
                    TestStep::new("Test the provider through the native button")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Test connection-Test",
                        ),
                )
                .with_step(
                    TestStep::new("Verify the model answered")
                        .set_timeout(Duration::from_secs(60))
                        .add_assertion(|app, window| {
                            if app
                                .read(|ctx| {
                                    ctx.element_position_by_id_at_last_frame(
                                        window,
                                        "mywarp-connection-verified",
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
                        .with_take_screenshot("provider-connection-verified.png"),
                );
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
                            profile.join("providers/config/opencode/opencode.jsonc"),
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
                        let preferences =
                            std::fs::read_to_string(profile.join("agent-preferences.json"))
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
                    TestStep::new("Set default model through Settings")
                        .with_click_on_saved_position(
                            "mywarp-provider-action-Use for new chats-DefaultModel",
                        ),
                )
                .with_step(
                    TestStep::new("Wait for default model to be saved").add_assertion(
                        move |_, _| {
                            let config = std::fs::read_to_string(
                                profile.join("providers/config/opencode/opencode.jsonc"),
                            )
                            .unwrap_or_default();
                            if config.contains("custom-mywarp/ui-fixture") {
                                AssertionOutcome::Success
                            } else {
                                AssertionOutcome::failure("Default model not yet saved".into())
                            }
                        },
                    ),
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
                    TestStep::new("Open providers from the chat footer")
                        .with_click_on_saved_position("mywarp-chat-providers"),
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
        let driver = builder.build("mywarp_provider_settings", false);
        warp::run_integration_test(driver)
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
