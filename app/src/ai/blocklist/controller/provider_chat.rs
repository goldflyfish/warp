use std::path::PathBuf;

use uuid::Uuid;
use warp_multi_agent_api::response_event::{StreamFinished, stream_finished};
use warp_multi_agent_api::{ResponseEvent, response_event};
use warp_util::path::EscapeChar;
use warpui::{AppContext, ModelContext, ModelHandle, SingletonEntity};

use super::{BlocklistAIController, BlocklistAIControllerEvent, InputQuery, InputQueryType};
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent_sdk::driver::harness::acp::desktop::{self, DesktopAcp};
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::ai::llms::LLMId;
use crate::persistence::model::ProviderChatSession;
use crate::settings::NativeChatRuntime;
use crate::terminal::model::session::SessionType;

impl BlocklistAIController {
    pub(crate) fn provider_escape_char(&self, ctx: &AppContext) -> EscapeChar {
        self.active_session
            .as_ref(ctx)
            .session(ctx)
            .map(|session| session.shell_family().escape_char())
            .unwrap_or(EscapeChar::Backslash)
    }
    pub(crate) fn provider_chat(&self) -> &ModelHandle<DesktopAcp> {
        &self.provider_chat
    }

    pub(crate) fn has_provider_chat(&self, id: AIConversationId, ctx: &AppContext) -> bool {
        BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&id)
            .is_some_and(|conversation| conversation.provider_session().is_some())
    }

    pub(crate) fn configure_provider_chat(
        &mut self,
        id: AIConversationId,
        runtime: NativeChatRuntime,
        ctx: &mut ModelContext<Self>,
    ) -> Result<(), String> {
        let history = BlocklistAIHistoryModel::handle(ctx);
        let conversation = history
            .as_ref(ctx)
            .conversation(&id)
            .ok_or("Conversation not found")?;
        if runtime == NativeChatRuntime::OpenCode
            && conversation
                .provider_session()
                .is_some_and(|session| session.runtime == "opencode")
        {
            self.ensure_provider_chat(id, ctx);
            return Ok(());
        }
        if conversation.exchange_count() != 0
            && !(warp_core::mywarp::enabled() && conversation.provider_session().is_none())
        {
            return Err("Start a new conversation to change its runtime.".to_owned());
        }
        match runtime {
            NativeChatRuntime::Warp => {
                if warp_core::mywarp::enabled() {
                    return Err(
                        "MyWarp uses your configured providers. Warp services are disabled."
                            .to_owned(),
                    );
                }
                self.provider_chat
                    .update(ctx, |model, ctx| model.disconnect(id, ctx));
                history.update(ctx, |history, ctx| {
                    history.set_provider_session(id, None, ctx)
                });
            }
            NativeChatRuntime::OpenCode => {
                let shared = self.terminal_model.lock().shared_session_status().clone();
                if shared.is_sharer() || shared.is_viewer() {
                    return Err("Native OpenCode chat is unavailable in shared sessions. Open a local tab instead.".to_owned());
                }
                let remote = matches!(
                    self.active_session.as_ref(ctx).session_type(ctx),
                    Some(SessionType::WarpifiedRemote { .. })
                );
                let wsl = self
                    .active_session
                    .as_ref(ctx)
                    .session(ctx)
                    .is_some_and(|session| session.is_wsl());
                let cwd = self
                    .active_session
                    .as_ref(ctx)
                    .current_working_directory()
                    .map(PathBuf::from)
                    .or_else(|| self.terminal_model.lock().session_startup_path())
                    .or_else(dirs::home_dir)
                    .unwrap_or_default();
                let config = ProviderChatSession {
                    binding_id: Uuid::new_v4().to_string(),
                    runtime: if remote || wsl {
                        "unsupported_remote_opencode"
                    } else {
                        "opencode"
                    }
                    .to_owned(),
                    cwd: cwd.to_string_lossy().into_owned(),
                    session_id: None,
                    model_id: None,
                };
                history.update(ctx, |history, ctx| {
                    history.set_provider_session(id, Some(config), ctx)
                });
                self.ensure_provider_chat(id, ctx);
            }
        }
        Ok(())
    }

    pub(crate) fn ensure_provider_chat(
        &mut self,
        id: AIConversationId,
        ctx: &mut ModelContext<Self>,
    ) {
        let config = BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&id)
            .and_then(|conversation| conversation.provider_session().cloned());
        if let Some(config) = config {
            let spawner = ctx.spawner();
            self.provider_chat
                .update(ctx, |model, ctx| model.connect(id, config, spawner, ctx));
        }
    }

    pub(crate) fn reconnect_provider_chat(
        &mut self,
        id: AIConversationId,
        ctx: &mut ModelContext<Self>,
    ) {
        if self
            .provider_chat
            .as_ref(ctx)
            .session(id)
            .is_some_and(|session| session.busy)
        {
            return;
        }
        self.provider_chat
            .update(ctx, |model, ctx| model.disconnect(id, ctx));
        self.ensure_provider_chat(id, ctx);
    }

    pub(super) fn try_send_provider_query(
        &mut self,
        id: AIConversationId,
        input: &InputQuery,
        is_queued_prompt: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if !self.has_provider_chat(id, ctx)
            && warp_core::mywarp::enabled()
            && let Err(error) = self.configure_provider_chat(id, NativeChatRuntime::OpenCode, ctx)
        {
            self.provider_chat
                .update(ctx, |model, ctx| model.set_error(id, error, ctx));
            return true;
        }
        if !self.has_provider_chat(id, ctx) {
            return false;
        }
        self.ensure_provider_chat(id, ctx);
        let error = match &input.input_query {
            InputQueryType::UserSubmittedQueryFromInput { .. } => {
                let context = self.context_model.as_ref(ctx);
                if !input.additional_attachments.is_empty()
                    || !context.pending_attachments().is_empty()
                    || !context.pending_context_block_ids().is_empty()
                    || context.pending_context_selected_text().is_some()
                {
                    Some("Attached context is not supported by native OpenCode chat yet. Paste the relevant text into the message instead.".to_owned())
                } else if self
                    .provider_chat
                    .as_ref(ctx)
                    .session(id)
                    .is_none_or(|session| !session.ready)
                {
                    self.provider_chat.as_ref(ctx).session(id).and_then(|session| session.error.clone()).or_else(|| Some("Connect OpenCode and choose an available model before sending. Your message has been kept in the input.".to_owned()))
                } else if self
                    .provider_chat
                    .as_ref(ctx)
                    .session(id)
                    .is_some_and(|session| session.busy)
                {
                    Some(
                        "The agent is busy. Stop the current turn before sending another message."
                            .to_owned(),
                    )
                } else {
                    None
                }
            }
            InputQueryType::AIInputType { .. } => Some(
                "This Warp-specific action is unavailable in an OpenCode conversation.".to_owned(),
            ),
        };
        if let Some(error) = error {
            self.provider_chat
                .update(ctx, |model, ctx| model.set_error(id, error, ctx));
            return true;
        }
        let (first, root, model_id) = {
            let history = BlocklistAIHistoryModel::as_ref(ctx);
            let conversation = history.conversation(&id).unwrap();
            let first = conversation.exchange_count() == 0;
            let root = if first {
                Uuid::new_v4().to_string()
            } else {
                conversation.get_root_task_id().to_string()
            };
            let model = conversation
                .provider_session()
                .and_then(|session| session.model_id.clone())
                .unwrap_or_else(|| "opencode".to_owned());
            (first, root, model)
        };
        let turn = match self.begin_external_harness_turn(id, None, ctx) {
            Ok(turn) => turn,
            Err(error) => {
                self.provider_chat.update(ctx, |model, ctx| {
                    model.set_error(id, error.to_string(), ctx)
                });
                return true;
            }
        };
        let text = input.query();
        let actions = desktop::opening_actions(root.clone(), turn.request_id.clone(), first, &text);
        self.apply_external_harness_event(
            &turn.stream_id,
            ResponseEvent {
                r#type: Some(response_event::Type::ClientActions(
                    response_event::ClientActions { actions },
                )),
            },
            ctx,
        );
        let accepted = self.provider_chat.update(ctx, |model, ctx| {
            model.submit(id, turn.clone(), root, text, ctx)
        });
        if !accepted {
            self.apply_external_harness_event(&turn.stream_id, ResponseEvent { r#type: Some(response_event::Type::Finished(StreamFinished { reason: Some(stream_finished::Reason::InternalError(stream_finished::InternalError { message: "The provider session disconnected; no fallback was used. Retry the connection.".to_owned() })), ..Default::default() })) }, ctx);
        }
        ctx.emit(BlocklistAIControllerEvent::SentRequest {
            contains_user_query: true,
            is_queued_prompt,
            model_id: LLMId::from(model_id),
            stream_id: turn.stream_id,
        });
        ctx.dispatch_global_action("workspace:save_app", ());
        true
    }
}
