//! Narrow assertions for the manual Windows native provider rendering check.
use warpui::{App, SingletonEntity as _, WindowId};

use crate::ai::agent::{AIAgentOutputStatus, AIAgentTextSection, FinishedAIAgentOutput};
use crate::ai::blocklist::history_model::BlocklistAIHistoryModel;
use crate::ai::execution_profiles::ExecutionProfileId;
use crate::menu::{Menu, MenuItem};
use crate::terminal::TerminalView;
use crate::view_components::dropdown::DropdownAction;
use crate::workspace::Workspace;

#[cfg(windows)]
pub fn profiles_are_ready(app: &App, window: WindowId) -> bool {
    crate::settings_view::native_profiles_ready(app, window)
        && app.read(|ctx| {
            crate::ai::llms::LLMPreferences::as_ref(ctx)
                .get_llm_info(&"custom-mywarp/hidden".into(), ctx)
                .is_some_and(|model| model.display_name.contains("Hidden model"))
        })
}

pub fn create_execution_profile_fixture(app: &mut App) {
    use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
    app.update(|ctx| {
        AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles, ctx| {
            let id = profiles.create_profile(ctx).expect("local profile");
            profiles.set_profile_name(&id, "Focused", ctx);
            profiles.set_base_model(&id, Some("custom-mywarp/hidden".into()), ctx);
        })
    });
}

pub fn open_default_execution_profile_editor(app: &mut App, window: WindowId) {
    let workspace = app
        .views_of_type::<Workspace>(window)
        .and_then(|views| views.first().cloned())
        .expect("workspace");
    workspace.update(app, |workspace, ctx| {
        workspace.open_execution_profile_editor_pane(
            None,
            ExecutionProfileId::default_profile(),
            ctx,
        );
    });
}

pub fn model_is_ready(app: &App, window: WindowId, model: &str) -> bool {
    let Some(workspace) = app
        .views_of_type::<Workspace>(window)
        .and_then(|views| views.first().cloned())
    else {
        return false;
    };
    let Some(terminal) = workspace.read(app, |workspace, ctx| workspace.active_terminal_id(ctx))
    else {
        return false;
    };
    app.read(|ctx| {
        BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(terminal)
            .and_then(|conversation| conversation.provider_session())
            .and_then(|session| session.model_id.as_deref())
            .is_some_and(|current| current == model)
    })
}

pub fn reasoning_is_ready(app: &App, window: WindowId, effort: &str) -> bool {
    let Some(workspace) = app
        .views_of_type::<Workspace>(window)
        .and_then(|views| views.first().cloned())
    else {
        return false;
    };
    let Some(terminal) = workspace.read(app, |workspace, ctx| workspace.active_terminal_id(ctx))
    else {
        return false;
    };
    app.read(|ctx| {
        let Some(conversation) = BlocklistAIHistoryModel::as_ref(ctx).active_conversation(terminal)
        else {
            return false;
        };
        let saved = conversation
            .provider_session()
            .is_some_and(|session| session.reasoning_effort.as_deref() == Some(effort));
        app.views_of_type::<TerminalView>(window)
            .is_some_and(|views| {
                views.iter().any(|view| {
                    view.as_ref(ctx)
                        .ai_controller()
                        .as_ref(ctx)
                        .provider_chat()
                        .as_ref(ctx)
                        .session(conversation.id())
                        .is_some_and(|session| {
                            session.ready
                                && !session.busy
                                && session.reasoning_effort.as_deref() == Some(effort)
                        })
                })
            })
            && saved
    })
}

pub fn model_menu_contains(app: &App, window: WindowId, model: &str) -> bool {
    app.views_of_type::<Menu<DropdownAction>>(window)
        .is_some_and(|menus| {
            menus.iter().any(|menu| {
                menu.read(app, |menu, _| {
                    menu.items().iter().any(
                        |item| matches!(item, MenuItem::Item(item) if item.label().contains(model)),
                    )
                })
            })
        })
}

pub fn model_menu_contains_only(app: &App, window: WindowId, model: &str) -> bool {
    app.views_of_type::<Menu<DropdownAction>>(window)
        .is_some_and(|menus| {
            menus.iter().any(|menu| {
                menu.read(app, |menu, _| {
                    let [MenuItem::Item(item)] = menu.items() else {
                        return false;
                    };
                    item.label().contains(model)
                })
            })
        })
}

pub fn history_contains_reply(app: &App, window: WindowId, expected: &str) -> bool {
    let Some(workspace) = app
        .views_of_type::<Workspace>(window)
        .and_then(|views| views.first().cloned())
    else {
        return false;
    };
    let Some(terminal) = workspace.read(app, |workspace, ctx| workspace.active_terminal_id(ctx))
    else {
        return false;
    };
    app.read(|ctx|BlocklistAIHistoryModel::as_ref(ctx).active_conversation(terminal)
        .is_some_and(|conversation|conversation.all_exchanges().into_iter().any(|exchange| {
            let AIAgentOutputStatus::Finished {finished_output:FinishedAIAgentOutput::Success {output}}=&exchange.output_status else {return false;};
            output.get().text_from_agent_output().any(|message|message.sections.iter().any(|section|matches!(section,AIAgentTextSection::PlainText {text} if text.text().contains(expected))))
        })))
}

pub fn chat_is_ready(app: &App, window: WindowId) -> bool {
    app.views_of_type::<TerminalView>(window)
        .is_some_and(|views| {
            views.iter().any(|handle| {
                handle.read(app, |view, ctx| {
                    BlocklistAIHistoryModel::as_ref(ctx)
                        .active_conversation(handle.id())
                        .and_then(|conversation| {
                            view.ai_controller()
                                .as_ref(ctx)
                                .provider_chat()
                                .as_ref(ctx)
                                .session(conversation.id())
                        })
                        .is_some_and(|session| session.ready && !session.busy)
                })
            })
        })
}

pub fn auto_approve_is_enabled(app: &App, window: WindowId) -> bool {
    app.views_of_type::<TerminalView>(window)
        .is_some_and(|views| {
            views.iter().any(|handle| {
                handle.read(app, |view, ctx| {
                    view.ai_controller()
                        .as_ref(ctx)
                        .provider_auto_approve_enabled(ctx)
                })
            })
        })
}

pub fn submit_auto_approve_fixture(app: &mut App, window: WindowId) {
    let workspace = app
        .views_of_type::<Workspace>(window)
        .and_then(|views| views.first().cloned())
        .expect("workspace");
    let terminal_id = workspace
        .read(app, |workspace, ctx| workspace.active_terminal_id(ctx))
        .expect("active terminal");
    let terminal = app
        .views_of_type::<TerminalView>(window)
        .and_then(|views| views.iter().find(|view| view.id() == terminal_id).cloned())
        .expect("terminal");
    let (controller, conversation) = terminal.read(app, |view, ctx| {
        (
            view.ai_controller().clone(),
            BlocklistAIHistoryModel::as_ref(ctx)
                .active_conversation_id(terminal_id)
                .expect("conversation"),
        )
    });
    controller.update(app, |controller, ctx| {
        controller.send_user_query_in_conversation_no_lrc_subagent(
            "Run the fixture read commands and file changes.".to_owned(),
            conversation,
            None,
            ctx,
        );
    });
}

#[cfg(windows)]
pub fn provider_key_is_masked(app: &App, window: WindowId) -> bool {
    crate::settings_view::native_provider_key_is_masked(app, window)
}

#[cfg(windows)]
pub fn configuration_is_open(app: &App, window: WindowId, key_file: bool) -> bool {
    app.views_of_type::<crate::code::view::CodeView>(window)
        .is_some_and(|views| {
            views.iter().any(|view| {
                view.read(app, |view, ctx| {
                    view.local_path(ctx).is_some_and(|path| {
                        if key_file {
                            path.extension().is_some_and(|extension| extension == "key")
                        } else {
                            path.file_name()
                                .is_some_and(|name| name == "opencode.jsonc")
                        }
                    })
                })
            })
        })
}

#[cfg(windows)]
pub fn form_is_ready(app: &App, window: WindowId) -> bool {
    crate::settings_view::native_provider_setup_ready(app, window)
}

#[cfg(windows)]
pub fn configuration_matches(
    app: &App,
    window: WindowId,
    model: &str,
    draft: &str,
    invalid: bool,
) -> bool {
    crate::settings_view::native_provider_configuration_matches(app, window, model, draft, invalid)
}

#[cfg(windows)]
pub fn chat_configuration_has_model(app: &App, model: &str) -> bool {
    app.models_of_type::<crate::ai::agent_sdk::driver::harness::acp::desktop::DesktopAcp>()
        .iter()
        .any(|handle| {
            handle.read(app, |model_state, _| {
                model_state.configuration_has_model(model)
            })
        })
}

struct AuxiliaryCheck {
    result: Option<Result<(), String>>,
}
impl warpui::Entity for AuxiliaryCheck {
    type Event = ();
}
impl warpui::SingletonEntity for AuxiliaryCheck {}

pub fn start_auxiliary_check(app: &mut App, window: WindowId) {
    let scope = app.read(|ctx| {
        crate::server::team_scope::RequestTeamScope::from_scope(
            &crate::workspaces::user_workspaces::UserWorkspaces::as_ref(ctx)
                .team_context_for_window(window),
        )
    });
    app.update(|ctx| {
        ctx.add_singleton_model(|ctx: &mut warpui::ModelContext<AuxiliaryCheck>| {
            let server = crate::server::server_api::ServerApiProvider::as_ref(ctx).get();
            ctx.spawn(
                async move { check_auxiliary_features(server.as_ref(), scope).await },
                |me, result, ctx| {
                    me.result = Some(result.map_err(|error| error.to_string()));
                    ctx.notify();
                },
            );
            AuxiliaryCheck { result: None }
        });
    });
}

pub fn auxiliary_check_result(app: &App) -> Option<Result<(), String>> {
    app.read(|ctx| AuxiliaryCheck::as_ref(ctx).result.clone())
}

async fn check_auxiliary_features(
    server: &crate::server::server_api::ServerApi,
    scope: crate::server::team_scope::RequestTeamScope,
) -> anyhow::Result<()> {
    use anyhow::ensure;

    use crate::ai::generate_block_title::api::GenerateBlockTitleRequest;
    use crate::ai::generate_code_review_content::api::{
        GenerateCodeReviewContentRequest, OutputType,
    };
    use crate::ai::predict::generate_ai_input_suggestions::GenerateAIInputSuggestionsRequest;
    use crate::ai::predict::generate_am_query_suggestions::GenerateAMQuerySuggestionsRequest;
    use crate::ai::predict::predict_am_queries::PredictAMQueriesRequest;
    use crate::server::server_api::ai::AIClient;
    use crate::server::server_api::block::BlockClient;

    let next = server
        .generate_ai_input_suggestions(&GenerateAIInputSuggestionsRequest::default(), scope)
        .await?;
    ensure!(
        !next.commands.is_empty() && !next.most_likely_action.is_empty(),
        "Next command generation returned no suggestion"
    );
    let prompt = server
        .generate_am_query_suggestions(&GenerateAMQuerySuggestionsRequest::default(), scope)
        .await?;
    ensure!(
        prompt.suggestion.is_some(),
        "Prompt suggestion was not returned"
    );
    let query = server
        .predict_am_queries(
            &PredictAMQueriesRequest {
                context_messages: vec![],
                partial_query: "Review".into(),
                system_context: None,
            },
            scope,
        )
        .await?;
    ensure!(
        !query.suggestion.is_empty(),
        "Query completion was not returned"
    );
    for output_type in [
        OutputType::CommitMessage,
        OutputType::PrTitle,
        OutputType::PrDescription,
    ] {
        let generated = server
            .generate_code_review_content(GenerateCodeReviewContentRequest {
                output_type,
                diff: "-before\n+after".into(),
                branch_name: "fixture".into(),
                commit_messages: vec![],
            })
            .await?;
        ensure!(
            !generated.content.is_empty(),
            "Code review generation returned no content"
        );
    }
    let title = server
        .generate_shared_block_title(GenerateBlockTitleRequest {
            command: "echo fixture".into(),
            output: "fixture".into(),
        })
        .await?;
    ensure!(
        !title.title.is_empty(),
        "Block title generation returned no title"
    );
    server
        .generate_commands_from_natural_language("Print a greeting".into(), None)
        .await
        .map_err(|_| anyhow::anyhow!("Command search failed"))?;
    server
        .generate_metadata_for_command("echo fixture".into())
        .await
        .map_err(|_| anyhow::anyhow!("Command metadata failed"))?;
    server
        .generate_dialogue_answer(vec![], "Hello".into(), None)
        .await?;
    let relevant = server
        .get_relevant_files(
            &crate::ai::get_relevant_files::api::GetRelevantFiles {
                query: "Review the fixture".into(),
                files: vec![crate::ai::get_relevant_files::api::FileContext {
                    path: "fixture.rs".into(),
                    symbols: "before".into(),
                }],
            },
            scope,
        )
        .await?;
    ensure!(
        relevant.relevant_file_paths == ["fixture.rs"],
        "Relevant-file selection must reject paths outside the supplied files"
    );
    crate::ai::blocklist::check_provider_code_suggestion().await?;
    let now = chrono::Utc::now().fixed_offset();
    let block = crate::server::block::Block {
        id: None,
        command: Some("echo <script>fixture</script>".into()),
        output: Some("fixture output".into()),
        stylized_command: None,
        stylized_output: None,
        stylized_prompt: None,
        stylized_prompt_and_command: None,
        pwd: None,
        time_started_term: now,
        time_completed_term: now,
    };
    let link = server
        .save_block(
            &block,
            Some("Native export".into()),
            false,
            crate::server::block::DisplaySetting::CommandAndOutput,
        )
        .await?;
    let path = url::Url::parse(&link)?
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Block export did not return a local file"))?;
    ensure!(
        std::fs::read_to_string(&path)?.contains("&lt;script&gt;"),
        "Block export must escape HTML"
    );
    let id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    ensure!(
        server
            .blocks_owned_by_user()
            .await?
            .iter()
            .any(|block| block.id.as_deref() == Some(id)),
        "Local export was not stored"
    );
    server.unshare_block(id.to_owned()).await?;
    ensure!(
        !path.exists(),
        "Removing a local export must remove its HTML file"
    );
    Ok(())
}
