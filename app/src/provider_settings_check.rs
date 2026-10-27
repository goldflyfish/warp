//! Narrow assertions for the manual Windows native provider rendering check.
use warpui::{App, SingletonEntity as _, WindowId};

use crate::ai::agent::{AIAgentOutputStatus, AIAgentTextSection, FinishedAIAgentOutput};
use crate::ai::blocklist::history_model::BlocklistAIHistoryModel;
use crate::workspace::Workspace;

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

#[cfg(windows)]
pub fn form_is_ready(app: &App, window: WindowId) -> bool {
    crate::settings_view::native_provider_setup_ready(app, window)
}
