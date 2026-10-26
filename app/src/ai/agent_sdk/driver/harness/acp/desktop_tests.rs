use futures::executor::block_on;
use futures::future::join;
use warpui::App;

use super::super::connection::tests::{connect, rejecting_handler};
use super::*;
use crate::ai::agent::PassiveSuggestionTrigger;
use crate::test_util::terminal::{add_window_with_terminal, initialize_app_for_terminal_view};

fn saved_session() -> ProviderChatSession {
    ProviderChatSession {
        binding_id: "binding".to_owned(),
        runtime: "opencode".to_owned(),
        cwd: "/project".to_owned(),
        session_id: Some("saved-session".to_owned()),
        model_id: Some("zai/model-a".to_owned()),
    }
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

fn waiting_session() -> DesktopSession {
    let (commands, _) = async_channel::unbounded();
    let (cancel, _) = async_channel::unbounded();
    DesktopSession {
        generation: Uuid::new_v4(),
        binding_id: "binding".to_owned(),
        session_id: Some("saved-session".to_owned()),
        models: Vec::new(),
        model_id: None,
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
    }
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
        let foreground = runtime.update(&mut app, |runtime, ctx| {
            runtime.sessions.insert(id, waiting_session());
            ctx.spawner()
        });
        let controller_spawner = controller.update(&mut app, |_, ctx| ctx.spawner());
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
            handler("session/request_permission", json!({"sessionId":"saved-session","toolCall":{"toolCallId":"call","title":"Run a command","kind":"execute","rawInput":{"command":"echo fixture"}},"options":[{"optionId":"broad","kind":"allow_always"},{"optionId":"once","kind":"allow_once"},{"optionId":"deny","kind":"reject_once"}]})).await
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
        let foreground = runtime.update(&mut app, |runtime, ctx| {
            let mut session = waiting_session();
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
        controller.update(&mut app, |controller, ctx| {
            controller.send_user_query_in_conversation("Hello provider".to_owned(), id, None, ctx);
        });
        let Command::Prompt {
            mut turn,
            root,
            text,
        } = rx.recv().await.unwrap()
        else {
            panic!("expected a native provider turn");
        };
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
        let id = AIConversationId::new();
        let mut session = waiting_session();
        let (reply, answer) = oneshot::channel();
        session.approvals.push(Approval {
            id: Uuid::new_v4(),
            title: "Write a file".to_owned(),
            details: "fixture".to_owned(),
            can_allow: true,
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
        let mut turn = waiting_session().active_turn.clone().unwrap();
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
        let mut session = waiting_session();
        session.approvals.push(Approval {
            id: Uuid::new_v4(),
            title: "Pending action".to_owned(),
            details: String::new(),
            can_allow: true,
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
