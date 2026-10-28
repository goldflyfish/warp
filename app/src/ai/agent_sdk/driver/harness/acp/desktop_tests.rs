use futures::executor::block_on;
use futures::future::join;
use warpui::App;

use super::super::connection::tests::{connect, rejecting_handler};
use super::*;
use crate::ai::agent::PassiveSuggestionTrigger;
use crate::terminal::event::BlockWorkingDirectoryUpdatedEvent;
use crate::terminal::model::block::BlockMetadata;
use crate::terminal::model::terminal_model::BlockIndex;
use crate::terminal::model_events::ModelEvent;
use crate::test_util::terminal::{add_window_with_terminal, initialize_app_for_terminal_view};

#[test]
fn provider_profiles_preserve_denied_tools_when_routing_approval_to_the_profile() {
    let mut permission = json!({"*":"allow", "bash":{"*":"allow", "rm *":"deny"}, "edit":"ask"});
    require_profile_approval(&mut permission);
    assert_eq!(
        permission,
        json!({"*":"ask", "bash":{"*":"ask", "rm *":"deny"}, "edit":"ask"})
    );
}

#[test]
fn provider_chat_identifies_read_only_tools() {
    let read = serde_json::from_value(json!({"toolCallId":"read", "kind":"read"})).unwrap();
    let search = serde_json::from_value(json!({"toolCallId":"search", "kind":"search"})).unwrap();
    let edit = serde_json::from_value(
        json!({"toolCallId":"edit", "kind":"edit", "locations":[{"path":"src/main.rs"}]}),
    )
    .unwrap();
    let execute = serde_json::from_value(json!({"toolCallId":"execute", "kind":"execute", "rawInput":{"command":"Get-ChildItem -Force -Path ."}})).unwrap();
    let write = serde_json::from_value(json!({"toolCallId":"write", "kind":"execute", "rawInput":{"command":"Set-Content -Path file.txt -Value changed"}})).unwrap();
    let missing_command =
        serde_json::from_value(json!({"toolCallId":"missing", "kind":"execute", "title":"ls"}))
            .unwrap();
    assert!(tool_is_read_only(
        &read,
        warp_util::path::EscapeChar::Backtick
    ));
    assert!(tool_is_read_only(
        &search,
        warp_util::path::EscapeChar::Backtick
    ));
    assert!(tool_is_read_only(
        &execute,
        warp_util::path::EscapeChar::Backtick
    ));
    assert!(!tool_is_read_only(
        &edit,
        warp_util::path::EscapeChar::Backtick
    ));
    assert!(!tool_is_read_only(
        &write,
        warp_util::path::EscapeChar::Backtick
    ));
    assert!(!tool_is_read_only(
        &missing_command,
        warp_util::path::EscapeChar::Backtick
    ));
}

#[test]
fn provider_chat_prompt_keeps_windows_directory_context_separate_from_the_user_query() {
    let content = prompt_content(
        "Is everything committed?".to_owned(),
        r"C:\Users\Public\Projects\project with spaces",
    )
    .unwrap();
    let content = serde_json::to_value(content).unwrap();

    assert!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .contains(r#""C:\\Users\\Public\\Projects\\project with spaces""#,)
    );
    assert_eq!(
        content[1],
        json!({"type":"text", "text":"Is everything committed?"})
    );
}

fn saved_session() -> ProviderChatSession {
    ProviderChatSession {
        binding_id: "binding".to_owned(),
        runtime: "opencode".to_owned(),
        cwd: "/project".to_owned(),
        session_id: Some("saved-session".to_owned()),
        model_id: Some("zai/model-a".to_owned()),
        reasoning_effort: None,
    }
}

#[test]
fn provider_profiles_do_not_fall_back_when_the_saved_model_is_unavailable() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let runtime = controller.read(&app, |controller, _| controller.provider_chat().clone());
        let history = BlocklistAIHistoryModel::handle(&app);
        let id = history.update(&mut app, |history, ctx| {
            let id = history.start_new_conversation(terminal.id(), false, false, false, ctx);
            history.set_provider_session(id, Some(saved_session()), ctx);
            id
        });
        let controller = controller.update(&mut app, |_, ctx| ctx.spawner());
        let (commands, receiver) = async_channel::unbounded();
        runtime.update(&mut app, |runtime, ctx| {
            let mut session = waiting_session(controller);
            session.commands = commands;
            session.busy = false;
            session.models = vec![ModelChoice {
                id: "zai/model-a".to_owned(),
                label: "Model A".to_owned(),
            }];
            runtime.sessions.insert(id, session);
            runtime.select_model(id, "unavailable/model".to_owned(), ctx);
        });
        runtime.read(&app, |runtime, _| {
            let session = runtime.session(id).unwrap();
            assert!(!session.ready);
            assert_eq!(session.model_id.as_deref(), Some("unavailable/model"));
            assert!(session.error.as_deref().unwrap().contains("no fallback"));
        });
        history.read(&app, |history, _| {
            assert_eq!(
                history
                    .conversation(&id)
                    .unwrap()
                    .provider_session()
                    .unwrap()
                    .model_id
                    .as_deref(),
                Some("unavailable/model")
            );
        });
        assert!(receiver.try_recv().is_err());
    });
}

#[test]
fn provider_chat_model_catalog_keeps_provider_ids_and_grouped_models() {
    let options = json!([{"id":"model-selector","category":"model","type":"select","currentValue":"zai/model-a","options":[{"name":"Z.ai","options":[{"value":"zai/model-a","name":"Model A"}]},{"value":"openai/model-b","name":"Model B"}]}]);
    let (config, choices, selected) = model_choices(&options);
    assert_eq!(config.as_deref(), Some("model-selector"));
    assert_eq!(selected.as_deref(), Some("zai/model-a"));
    assert_eq!(
        choices,
        vec![
            ModelChoice {
                id: "zai/model-a".to_owned(),
                label: "Model A (zai/model-a)".to_owned()
            },
            ModelChoice {
                id: "openai/model-b".to_owned(),
                label: "Model B (openai/model-b)".to_owned()
            }
        ]
    );
}

#[test]
fn provider_chat_attended_config_retains_provider_configuration() {
    let original = r#"{"provider":{"fixture":{"options":{"baseURL":"http://localhost:1","apiKey":"dummy"}}},"permission":"allow","model":"fixture/demo"}"#;
    let configured: Value =
        serde_json::from_str(&attended_config(Some(original)).unwrap()).unwrap();
    assert_eq!(configured["permission"], "ask");
    assert_eq!(
        configured["provider"]["fixture"]["options"]["apiKey"],
        "dummy"
    );
    assert_eq!(configured["model"], "fixture/demo");
    let unchanged: Value = serde_json::from_str(original).unwrap();
    assert_eq!(unchanged["permission"], "allow");
}

fn reasoning_options(current: &str) -> Value {
    json!([
        {"id":"model","category":"model","type":"select","currentValue":"zai/model-a","options":[{"value":"zai/model-a","name":"Model A"}]},
        {"id":"provider-effort","category":"thought_level","type":"select","currentValue":current,"options":[{"name":"Effort","options":[{"value":"none","name":"Off"},{"value":"thinking","name":"Thinking"}]}]}
    ])
}

#[test]
fn provider_chat_reasoning_preserves_provider_values_and_labels() {
    let (id, choices, selected) = setting_choices(&reasoning_options("thinking"), "thought_level");

    assert_eq!(id.as_deref(), Some("provider-effort"));
    assert_eq!(selected.as_deref(), Some("thinking"));
    assert_eq!(
        choices,
        vec![
            ModelChoice {
                id: "none".into(),
                label: "Off".into()
            },
            ModelChoice {
                id: "thinking".into(),
                label: "Thinking".into()
            },
        ]
    );
}

#[test]
fn provider_chat_model_without_reasoning_does_not_invent_efforts() {
    let options = json!([{"id":"model","category":"model","type":"select","currentValue":"zai/model-a","options":[]}]);

    assert_eq!(
        setting_choices(&options, "thought_level"),
        (None, vec![], None)
    );
}

#[test]
fn provider_chat_reasoning_change_uses_advertised_setting_and_requires_confirmation() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let options = reasoning_options("none");
        let client = set_reasoning_effort(&connection, "saved-session", &options, "thinking");
        let server = async {
            let request = agent.next_message().await;
            assert_eq!(request["method"], "session/set_config_option");
            assert_eq!(
                request["params"],
                json!({"sessionId":"saved-session","configId":"provider-effort","value":"thinking"})
            );
            agent.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"configOptions":reasoning_options("thinking")}})).await;
        };

        let (response, ()) = join(client, server).await;
        assert_eq!(response.unwrap(), reasoning_options("thinking"));
    });
}

#[test]
fn provider_chat_unsupported_reasoning_is_rejected_without_a_fallback_rpc() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let error = set_reasoning_effort(
            &connection,
            "saved-session",
            &reasoning_options("none"),
            "high",
        )
        .await
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not offer reasoning effort")
        );
        assert!(futures::poll!(Box::pin(agent.next_message())).is_pending());
    });
}

#[test]
fn provider_chat_unconfirmed_reasoning_change_is_rejected() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let options = reasoning_options("none");
        let client = set_reasoning_effort(&connection, "saved-session", &options, "thinking");
        let server = async {
            let request = agent.next_message().await;
            agent.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"configOptions":reasoning_options("none")}})).await;
        };

        let (response, ()) = join(client, server).await;
        assert!(
            response
                .unwrap_err()
                .to_string()
                .contains("did not confirm the requested reasoning effort")
        );
    });
}

#[test]
fn provider_chat_reasoning_change_cannot_silently_switch_models() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let options = reasoning_options("none");
        let client = set_reasoning_effort(&connection, "saved-session", &options, "thinking");
        let server = async {
            let request = agent.next_message().await;
            let mut changed = reasoning_options("thinking");
            changed[0]["currentValue"] = json!("other/model");
            agent
                .send(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":{"configOptions":changed}}),
                )
                .await;
        };

        let (response, ()) = join(client, server).await;
        assert!(
            response
                .unwrap_err()
                .to_string()
                .contains("did not confirm the requested reasoning effort and model")
        );
    });
}

#[test]
fn provider_chat_older_sessions_restore_without_a_reasoning_preference() {
    let session: ProviderChatSession = serde_json::from_value(json!({
        "binding_id":"binding", "runtime":"opencode", "cwd":"/project",
        "session_id":"saved-session", "model_id":"zai/model-a"
    }))
    .unwrap();

    assert_eq!(session.reasoning_effort, None);
    assert_eq!(session.model_id.as_deref(), Some("zai/model-a"));
}

#[test]
fn provider_chat_invalid_inline_config_is_rejected_without_replacing_it() {
    assert!(attended_config(Some("not-json")).is_err());
    assert!(attended_config(Some("[]")).is_err());
}

#[test]
fn provider_chat_resume_loads_the_saved_runtime_session_and_folder() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let config = saved_session();
        let client = initialize(&connection, &config);
        let server = async {
            let init = agent.next_message().await;
            assert_eq!(init["method"], "initialize");
            assert_eq!(
                init["params"]["clientCapabilities"]["fs"]["writeTextFile"],
                false
            );
            agent.send(json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}})).await;
            let load = agent.next_message().await;
            assert_eq!(load["method"], "session/load");
            assert_eq!(load["params"]["sessionId"], "saved-session");
            assert_eq!(load["params"]["cwd"], "/project");
            agent
                .send(json!({"jsonrpc":"2.0","id":load["id"],"result":{"configOptions":[]}}))
                .await;
        };
        let (response, ()) = join(client, server).await;
        assert_eq!(response.unwrap().0, "saved-session");
    });
}

#[test]
fn provider_chat_resume_never_creates_a_replacement_session_when_load_is_unsupported() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let config = saved_session();
        let client = initialize(&connection, &config);
        let server = async {
            let init = agent.next_message().await;
            agent.send(json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":false}}})).await;
        };
        let (response, ()) = join(client, server).await;
        assert!(
            response
                .unwrap_err()
                .to_string()
                .contains("cannot reload the saved session")
        );
        assert!(futures::poll!(Box::pin(agent.next_message())).is_pending());
    });
}

#[test]
fn provider_chat_missing_saved_model_is_rejected_without_a_fallback_rpc() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let options = json!([{"id":"model","category":"model","type":"select","currentValue":"other/model","options":[{"value":"other/model"}]}]);
        let error = set_model(&connection, "saved-session", &options, "zai/missing")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("does not offer the saved model"));
        assert!(futures::poll!(Box::pin(agent.next_message())).is_pending());
    });
}

#[test]
fn provider_chat_model_change_requires_confirmation_of_the_exact_model() {
    let (connection, _, mut agent) = connect(rejecting_handler());
    block_on(async {
        let options = json!([{"id":"model","category":"model","type":"select","currentValue":"old/model","options":[{"value":"zai/model-a"}]}]);
        let client = set_model(&connection, "saved-session", &options, "zai/model-a");
        let server = async {
            let request = agent.next_message().await;
            assert_eq!(request["params"]["value"], "zai/model-a");
            agent.send(json!({"jsonrpc":"2.0","id":request["id"],"result":{"configOptions":[{"id":"model","category":"model","type":"select","currentValue":"wrong/model","options":[]}]}})).await;
        };
        let (response, ()) = join(client, server).await;
        assert!(
            response
                .unwrap_err()
                .to_string()
                .contains("did not confirm the requested model")
        );
    });
}

fn waiting_session(controller: ModelSpawner<BlocklistAIController>) -> DesktopSession {
    #[cfg(not(windows))]
    drop(controller);
    let (commands, _) = async_channel::unbounded();
    let (cancel, _) = async_channel::unbounded();
    DesktopSession {
        generation: Uuid::new_v4(),
        binding_id: "binding".to_owned(),
        session_id: Some("saved-session".to_owned()),
        models: Vec::new(),
        model_id: None,
        reasoning_efforts: Vec::new(),
        reasoning_effort: None,
        busy: true,
        ready: true,
        status: "Working".to_owned(),
        error: None,
        approvals: Vec::new(),
        commands,
        cancel,
        active_turn: Some(ExternalHarnessTurn {
            stream_id: crate::ai::blocklist::ResponseStreamId::for_shared_session(
                &warp_multi_agent_api::response_event::StreamInit {
                    conversation_id: "local".to_owned(),
                    request_id: "request".to_owned(),
                    ..Default::default()
                },
            ),
            request_id: "request".to_owned(),
        }),
        task: None,
        #[cfg(windows)]
        controller,
        #[cfg(windows)]
        connecting: false,
        #[cfg(windows)]
        reload_pending: false,
    }
}

#[test]
fn provider_chat_auto_approve_persists_the_active_profiles_write_permissions() {
    use warpui::TypedActionView as _;

    use crate::ai::execution_profiles::ActionPermission;
    use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
    use crate::settings::AgentModeCommandExecutionPredicate;
    use crate::terminal::view::TerminalAction;

    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let runtime = controller.read(&app, |controller, _| controller.provider_chat().clone());
        let controller_spawner = controller.update(&mut app, |_, ctx| ctx.spawner());
        let history = BlocklistAIHistoryModel::handle(&app);
        let id = history.update(&mut app, |history, ctx| {
            history.start_new_conversation(terminal.id(), false, false, false, ctx)
        });
        let (reply, mut answer) = oneshot::channel();
        let (other_reply, mut other_answer) = oneshot::channel();
        runtime.update(&mut app, |runtime, _| {
            let mut session = waiting_session(controller_spawner);
            session.approvals.push(Approval {
                id: Uuid::new_v4(),
                title: "Write a file".to_owned(),
                details: String::new(),
                can_allow: true,
                kind: Some(protocol::ToolKind::Execute),
                reply,
            });
            session.approvals.push(Approval {
                id: Uuid::new_v4(),
                title: "Another tool".to_owned(),
                details: String::new(),
                can_allow: true,
                kind: Some(protocol::ToolKind::Other),
                reply: other_reply,
            });
            runtime.sessions.insert(id, session);
        });
        history.update(&mut app, |history, ctx| {
            history.set_provider_session(id, Some(saved_session()), ctx);
            history.set_active_conversation_id(id, terminal.id(), ctx);
        });
        controller.read(&app, |controller, ctx| {
            assert!(!controller.provider_auto_approve_enabled(ctx));
        });
        terminal.update(&mut app, |terminal, ctx| {
            terminal.handle_action(&TerminalAction::ToggleAutoexecuteMode, ctx)
        });
        controller.read(&app, |controller, ctx| {
            assert!(controller.provider_auto_approve_enabled(ctx));
        });
        assert_eq!(answer.try_recv().unwrap(), Some(true));
        assert_eq!(other_answer.try_recv().unwrap(), None);
        let profiles = AIExecutionProfilesModel::handle(&app);
        let profile_id = profiles.read(&app, |profiles, ctx| {
            profiles
                .active_profile(Some(terminal.id()), ctx)
                .id()
                .clone()
        });
        let restored = app.add_model(|ctx| {
            AIExecutionProfilesModel::new(&crate::LaunchMode::new_for_unit_test(), ctx)
        });
        restored.read(&app, |profiles, ctx| {
            let profile = profiles.get_profile_by_id(&profile_id, ctx).unwrap();
            assert_eq!(
                profile.data().execute_commands,
                ActionPermission::AlwaysAllow
            );
            assert_eq!(
                profile.data().apply_code_diffs,
                ActionPermission::AlwaysAllow
            );
        });
        profiles.update(&mut app, |profiles, ctx| {
            profiles.add_to_command_denylist(
                &profile_id,
                &AgentModeCommandExecutionPredicate::new_regex("blocked-command").unwrap(),
                ctx,
            );
        });
        let denylist = profiles.read(&app, |profiles, ctx| {
            profiles
                .get_profile_by_id(&profile_id, ctx)
                .unwrap()
                .data()
                .command_denylist
                .clone()
        });
        controller.read(&app, |controller, ctx| {
            let call = serde_json::from_value(json!({"toolCallId":"write", "kind":"execute", "rawInput":{"command":"echo content > file.txt"}})).unwrap();
            assert!(profile_allows_tool(controller, id, terminal.id(), &call, None, ctx));
            let denied = serde_json::from_value(json!({"toolCallId":"denied", "kind":"execute", "rawInput":{"command":"blocked-command"}})).unwrap();
            assert!(!profile_allows_tool(controller, id, terminal.id(), &denied, None, ctx));
        });
        controller.update(&mut app, |controller, ctx| {
            controller
                .set_provider_auto_approve(id, false, ctx)
                .unwrap();
            assert!(!controller.provider_auto_approve_enabled(ctx));
        });
        profiles.read(&app, |profiles, ctx| {
            let profile = profiles.get_profile_by_id(&profile_id, ctx).unwrap();
            assert_eq!(profile.data().execute_commands, ActionPermission::AlwaysAsk);
            assert_eq!(profile.data().apply_code_diffs, ActionPermission::AlwaysAsk);
            assert_eq!(profile.data().command_denylist, denylist);
        });
    });
}

#[test]
fn provider_chat_permission_waits_for_an_exact_allow_once_decision() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let id = BlocklistAIHistoryModel::handle(&app).update(&mut app, |history, ctx| {
            history.start_new_conversation(terminal.id(), false, false, false, ctx)
        });
        let runtime = controller.read(&app, |controller, _| controller.provider_chat().clone());
        let controller_spawner = controller.update(&mut app, |_, ctx| ctx.spawner());
        let foreground = runtime.update(&mut app, |runtime, ctx| {
            runtime
                .sessions
                .insert(id, waiting_session(controller_spawner.clone()));
            ctx.spawner()
        });
        let handler = permission_handler(
            id,
            "binding".to_owned(),
            terminal.id(),
            foreground,
            controller_spawner,
        );
        let (pending_tx, pending_rx) = async_channel::bounded(1);
        let observed = runtime.clone();
        app.update(|ctx| {
            ctx.subscribe_to_model(&runtime, move |_, _, ctx| {
                if let Some(approval) = observed
                    .as_ref(ctx)
                    .session(id)
                    .and_then(|session| session.approvals.first())
                {
                    let _ = pending_tx.try_send(approval.id);
                }
            })
        });
        let (result_tx, mut result_rx) = oneshot::channel();
        let task = runtime.update(&mut app, |_, ctx| ctx.spawn(async move {
            handler("session/request_permission", json!({"sessionId":"saved-session","toolCall":{"toolCallId":"call","title":"Run a command","kind":"execute","rawInput":{"command":"echo fixture > fixture.txt"}},"options":[{"optionId":"broad","kind":"allow_always"},{"optionId":"once","kind":"allow_once"},{"optionId":"deny","kind":"reject_once"}]})).await
        }, move |_, result, _| { let _ = result_tx.send(result); }));
        let approval = pending_rx.recv().await.unwrap();
        assert!(result_rx.try_recv().unwrap().is_none());
        runtime.update(&mut app, |runtime, ctx| {
            runtime.answer(id, Uuid::nil(), true, ctx)
        });
        assert!(result_rx.try_recv().unwrap().is_none());
        runtime.update(&mut app, |runtime, ctx| {
            runtime.answer(id, approval, true, ctx)
        });
        app.update(|ctx| ctx.await_spawned_future(task.future_id()))
            .await;
        let result = result_rx.await.unwrap().unwrap();
        assert_eq!(result["outcome"]["optionId"], "once");
    });
}

#[test]
fn provider_chat_stream_renders_in_native_history_without_a_warp_server_identity() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let id = BlocklistAIHistoryModel::handle(&app).update(&mut app, |history, ctx| {
            history.start_new_conversation(terminal.id(), false, false, false, ctx)
        });
        let runtime = controller.read(&app, |controller, _| controller.provider_chat().clone());
        let (tx, rx) = async_channel::unbounded();
        let controller_spawner = controller.update(&mut app, |_, ctx| ctx.spawner());
        let foreground = runtime.update(&mut app, |runtime, ctx| {
            let mut session = waiting_session(controller_spawner);
            session.commands = tx;
            session.busy = false;
            session.active_turn = None;
            runtime.sessions.insert(id, session);
            ctx.spawner()
        });
        BlocklistAIHistoryModel::handle(&app).update(&mut app, |history, ctx| {
            history.set_provider_session(id, Some(saved_session()), ctx);
            history.set_active_conversation_id(id, terminal.id(), ctx);
        });
        controller.update(&mut app, |controller, ctx| {
            assert!(
                controller
                    .build_passive_suggestions_request_params(
                        Some(id),
                        PassiveSuggestionTrigger::FilesChanged,
                        vec![],
                        ctx
                    )
                    .is_err()
            );
            assert!(
                controller
                    .build_passive_suggestions_request_params(
                        None,
                        PassiveSuggestionTrigger::FilesChanged,
                        vec![],
                        ctx
                    )
                    .is_err()
            );
        });
        let dispatcher = terminal.read(&app, |terminal, _| {
            terminal.model_event_dispatcher().clone()
        });
        dispatcher.update(&mut app, |_, ctx| {
            ctx.emit(ModelEvent::BlockWorkingDirectoryUpdated(
                BlockWorkingDirectoryUpdatedEvent {
                    block_metadata: BlockMetadata::new(None, Some("/new-project".to_owned())),
                    block_index: BlockIndex::zero(),
                    is_for_in_band_command: false,
                    is_done_bootstrapping: true,
                },
            ));
        });
        controller.update(&mut app, |controller, ctx| {
            controller.send_user_query_in_conversation("Hello provider".to_owned(), id, None, ctx);
        });
        let Command::Prompt {
            mut turn,
            root,
            text,
            cwd,
        } = rx.recv().await.unwrap()
        else {
            panic!("expected a native provider turn");
        };
        assert_eq!(cwd.as_deref(), Some("/new-project"));
        controller.update(&mut app, |controller, ctx| {
            controller.send_user_query_in_conversation("Must wait".to_owned(), id, None, ctx);
        });
        assert!(rx.try_recv().is_err());
        assert!(runtime.read(&app, |runtime, _| {
            runtime
                .session(id)
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("busy")
        }));
        let controller_spawner = controller.update(&mut app, |_, ctx| ctx.spawner());
        let (connection, notifications, mut agent) = connect(rejecting_handler());
        let connection = Arc::new(connection);
        let (_cancel, cancels) = async_channel::unbounded();
        let task = runtime.update(&mut app, |_, ctx| {
            ctx.spawn(
                async move {
                    let mut mapper =
                        AcpTurnMapper::new(root, turn.request_id.clone(), HashMap::new());
                    let reason = prompt(
                        id,
                        &connection,
                        &notifications,
                        &cancels,
                        &saved_session(),
                        &mut mapper,
                        &mut turn,
                        &controller_spawner,
                        &foreground,
                        text,
                        cwd,
                    )
                    .await
                    .unwrap();
                    finish(&controller_spawner, &turn, stream_finished_for(reason))
                        .await
                        .unwrap();
                },
                |_, _, _| {},
            )
        });
        let completion = app.update(|ctx| ctx.await_spawned_future(task.future_id()));
        let server = async {
            let request = agent.next_message().await;
            assert_eq!(request["method"], "session/prompt");
            assert_eq!(request["params"]["sessionId"], "saved-session");
            assert!(
                request["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("/new-project")
            );
            assert_eq!(request["params"]["prompt"][1]["text"], "Hello provider");
            agent.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"wrong-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Wrong session"}}}})).await;
            agent.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"saved-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Native provider reply"}}}})).await;
            agent
                .send(
                    json!({"jsonrpc":"2.0","id":request["id"],"result":{"stopReason":"end_turn"}}),
                )
                .await;
        };
        join(completion, server).await;
        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            let conversation = history.conversation(&id).unwrap();
            assert_eq!(conversation.status(), &crate::ai::agent::conversation::ConversationStatus::Success);
            assert_eq!(conversation.driver(), crate::ai::agent::conversation::ConversationDriver::ExternalHarness);
            assert!(conversation.server_conversation_token().is_none());
            assert!(!conversation.to_conversation_data().tasks.iter().flat_map(|task| &task.messages).any(|message| matches!(&message.message, Some(warp_multi_agent_api::message::Message::AgentOutput(output)) if output.text.contains("Wrong session"))));
            assert!(conversation.to_conversation_data().tasks.iter().flat_map(|task| &task.messages).any(|message| matches!(&message.message, Some(warp_multi_agent_api::message::Message::AgentOutput(output)) if output.text == "Native provider reply")));
        });
        controller.update(&mut app, |controller, ctx| {
            controller
                .configure_provider_chat(id, crate::settings::NativeChatRuntime::OpenCode, ctx)
                .unwrap();
        });
        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            assert_eq!(
                history
                    .conversation(&id)
                    .unwrap()
                    .provider_session()
                    .unwrap()
                    .session_id,
                saved_session().session_id
            );
        });
    });
}

#[test]
fn provider_chat_clearing_a_pending_approval_rejects_it() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let controller = controller.update(&mut app, |_, ctx| ctx.spawner());
        let id = AIConversationId::new();
        let mut session = waiting_session(controller);
        let (reply, answer) = oneshot::channel();
        session.approvals.push(Approval {
            id: Uuid::new_v4(),
            title: "Write a file".to_owned(),
            details: "fixture".to_owned(),
            can_allow: true,
            kind: Some(protocol::ToolKind::Edit),
            reply,
        });
        let model = app.add_model(|ctx| DesktopAcp::new(EntityId::new(), ctx));
        model.update(&mut app, |model, ctx| {
            model.sessions.insert(id, session);
            model.cancel(id, ctx);
        });
        assert!(answer.await.is_err());
        assert_eq!(
            model.read(&app, |model, _| model.session(id).unwrap().approvals.len()),
            0
        );
    });
}

#[test]
fn provider_chat_stop_sends_session_cancel_and_waits_for_the_runtime() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let runtime = controller.read(&app, |controller, _| controller.provider_chat().clone());
        let foreground = runtime.update(&mut app, |_, ctx| ctx.spawner());
        let controller = controller.update(&mut app, |_, ctx| ctx.spawner());
        let mut turn = waiting_session(controller.clone())
            .active_turn
            .clone()
            .unwrap();
        let (connection, notifications, mut agent) = connect(rejecting_handler());
        let connection = Arc::new(connection);
        let (cancel, cancels) = async_channel::unbounded();
        cancel.try_send(()).unwrap();
        let (result_tx, result_rx) = oneshot::channel();
        let task = runtime.update(&mut app, |_, ctx| {
            ctx.spawn(
                async move {
                    let mut mapper = AcpTurnMapper::new(
                        "root".to_owned(),
                        turn.request_id.clone(),
                        HashMap::new(),
                    );
                    prompt(
                        AIConversationId::new(),
                        &connection,
                        &notifications,
                        &cancels,
                        &saved_session(),
                        &mut mapper,
                        &mut turn,
                        &controller,
                        &foreground,
                        "Stop this turn".to_owned(),
                        None,
                    )
                    .await
                },
                move |_, result, _| {
                    let _ = result_tx.send(result);
                },
            )
        });
        let completion = app.update(|ctx| ctx.await_spawned_future(task.future_id()));
        let server = async {
            let prompt = agent.next_message().await;
            assert_eq!(prompt["method"], "session/prompt");
            assert!(
                prompt["params"]["prompt"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("/project")
            );
            let cancel = agent.next_message().await;
            assert_eq!(cancel["method"], "session/cancel");
            assert_eq!(cancel["params"]["sessionId"], "saved-session");
            agent
                .send(
                    json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"cancelled"}}),
                )
                .await;
        };
        join(completion, server).await;
        assert_eq!(result_rx.await.unwrap().unwrap(), StopReason::Cancelled);
    });
}

#[test]
fn provider_chat_removing_a_conversation_drops_its_pending_approvals() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let controller = terminal.read(&app, |terminal, _| terminal.ai_controller().clone());
        let runtime = controller.read(&app, |controller, _| controller.provider_chat().clone());
        let history = BlocklistAIHistoryModel::handle(&app);
        let id = history.update(&mut app, |history, ctx| {
            history.start_new_conversation(terminal.id(), false, false, false, ctx)
        });
        let (reply, answer) = oneshot::channel();
        let controller = controller.update(&mut app, |_, ctx| ctx.spawner());
        let mut session = waiting_session(controller);
        session.approvals.push(Approval {
            id: Uuid::new_v4(),
            title: "Pending action".to_owned(),
            details: String::new(),
            can_allow: true,
            kind: Some(protocol::ToolKind::Edit),
            reply,
        });
        runtime.update(&mut app, |runtime, _| {
            runtime.sessions.insert(id, session);
        });
        history.update(&mut app, |history, ctx| {
            history.remove_conversation(id, terminal.id(), ctx)
        });
        assert!(answer.await.is_err());
        assert!(runtime.read(&app, |runtime, _| runtime.session(id).is_none()));
    });
}
