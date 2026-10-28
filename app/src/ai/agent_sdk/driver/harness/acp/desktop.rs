use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use futures::channel::oneshot;
use futures::future::{Either, select};
use futures::{FutureExt as _, pin_mut, select_biased};
use serde_json::{Value, json};
use uuid::Uuid;
use warp_multi_agent_api::response_event::{StreamFinished, stream_finished};
use warp_multi_agent_api::{ClientAction, ResponseEvent, response_event};
use warpui::r#async::executor::Background;
use warpui::r#async::{SpawnedFutureHandle, Timer};
use warpui::{Entity, EntityId, ModelContext, ModelSpawner, SingletonEntity};

use super::connection::{AcpConnection, AgentRequestHandler, InboundNotification, RpcError};
use super::mapping::{AcpTurnMapper, TurnEvent, effective_tool_kind};
use super::policy::{PolicyDecision, PolicyRequest};
use super::protocol::{
    self, ContentBlock, InitializeResponse, PermissionOptionKind, PermissionOutcome, PromptParams,
    PromptResponse, RequestPermissionParams, RequestPermissionResponse, SessionUpdateParams,
    StopReason,
};
use super::turn_driver::stream_finished_for;
use crate::ai::agent::CancellationReason;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::{
    BlocklistAIController, BlocklistAIHistoryEvent, BlocklistAIHistoryModel,
    BlocklistAIPermissions, ExternalHarnessTurn,
};
use crate::persistence::model::ProviderChatSession;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ModelChoice {
    pub id: String,
    pub label: String,
}

pub(crate) struct Approval {
    pub id: Uuid,
    pub title: String,
    pub details: String,
    pub can_allow: bool,
    kind: Option<protocol::ToolKind>,
    reply: oneshot::Sender<bool>,
}

impl Approval {
    pub(crate) fn can_auto_approve(&self) -> bool {
        self.can_allow
            && matches!(
                self.kind,
                Some(
                    protocol::ToolKind::Execute
                        | protocol::ToolKind::Edit
                        | protocol::ToolKind::Delete
                        | protocol::ToolKind::Move
                )
            )
    }
}

pub(crate) struct DesktopSession {
    generation: Uuid,
    binding_id: String,
    session_id: Option<String>,
    pub models: Vec<ModelChoice>,
    pub model_id: Option<String>,
    pub reasoning_efforts: Vec<ModelChoice>,
    pub reasoning_effort: Option<String>,
    pub busy: bool,
    pub ready: bool,
    pub status: String,
    pub error: Option<String>,
    pub approvals: Vec<Approval>,
    commands: async_channel::Sender<Command>,
    cancel: async_channel::Sender<()>,
    active_turn: Option<ExternalHarnessTurn>,
    task: Option<SpawnedFutureHandle>,
    #[cfg(windows)]
    controller: ModelSpawner<BlocklistAIController>,
    #[cfg(windows)]
    connecting: bool,
    #[cfg(windows)]
    reload_pending: bool,
}

impl Drop for DesktopSession {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

pub(crate) struct DesktopAcp {
    terminal_id: EntityId,
    sessions: HashMap<AIConversationId, DesktopSession>,
}

impl Entity for DesktopAcp {
    type Event = ();
}

impl DesktopAcp {
    pub(crate) fn new(terminal_id: EntityId, ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(
            &BlocklistAIHistoryModel::handle(ctx),
            |me, _, event, ctx| {
                if let BlocklistAIHistoryEvent::RemoveConversation {
                    conversation_id, ..
                }
                | BlocklistAIHistoryEvent::DeletedConversation {
                    conversation_id, ..
                } = event
                {
                    me.disconnect(*conversation_id, ctx);
                }
            },
        );
        #[cfg(windows)]
        if ctx.has_singleton_model::<crate::configuration_watcher::ConfigurationWatcher>() {
            ctx.subscribe_to_model(
                &crate::configuration_watcher::ConfigurationWatcher::handle(ctx),
                |me, _, event, ctx| match event {
                    crate::configuration_watcher::ConfigurationEvent::Changed => {
                        for session in me.sessions.values_mut() {
                            session.reload_pending = true;
                        }
                        me.reload_configuration(ctx);
                    }
                    crate::configuration_watcher::ConfigurationEvent::Invalid(error) => {
                        for session in me.sessions.values_mut() {
                            session.reload_pending = false;
                            session.error =
                                Some(format!("Configuration was not reloaded: {error}"));
                        }
                        ctx.emit(());
                    }
                    crate::configuration_watcher::ConfigurationEvent::Checked => {}
                },
            );
        }
        Self {
            terminal_id,
            sessions: HashMap::new(),
        }
    }

    #[cfg(windows)]
    fn reload_configuration(&mut self, ctx: &mut ModelContext<Self>) {
        let sessions: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, session)| session.reload_pending && !session.busy && !session.connecting)
            .map(|(id, session)| (*id, session.controller.clone()))
            .collect();
        for (id, controller) in sessions {
            let config = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&id)
                .and_then(|conversation| conversation.provider_session().cloned());
            if let Some(config) = config {
                self.sessions.remove(&id);
                self.connect(id, config, controller, ctx);
            }
        }
    }

    #[cfg(all(windows, feature = "provider-settings-check"))]
    pub(crate) fn configuration_has_model(&self, model: &str) -> bool {
        self.sessions.values().any(|session| {
            session.ready
                && !session.connecting
                && !session.reload_pending
                && session.models.iter().any(|choice| choice.id == model)
        })
    }

    pub(crate) fn session(&self, id: AIConversationId) -> Option<&DesktopSession> {
        self.sessions.get(&id)
    }

    pub(crate) fn is_connected(&self, id: AIConversationId) -> bool {
        self.sessions
            .get(&id)
            .is_some_and(|session| !session.commands.is_closed())
    }

    pub(crate) fn can_stop(&self, id: AIConversationId) -> bool {
        self.sessions
            .get(&id)
            .is_some_and(|session| session.active_turn.is_some())
    }

    pub(crate) fn disconnect(&mut self, id: AIConversationId, ctx: &mut ModelContext<Self>) {
        self.sessions.remove(&id);
        ctx.emit(());
    }

    pub(crate) fn set_error(
        &mut self,
        id: AIConversationId,
        message: String,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&id) {
            session.error = Some(message);
        }
        ctx.emit(());
    }

    pub(crate) fn connect(
        &mut self,
        id: AIConversationId,
        config: ProviderChatSession,
        controller: ModelSpawner<BlocklistAIController>,
        ctx: &mut ModelContext<Self>,
    ) {
        if self
            .sessions
            .get(&id)
            .is_some_and(|session| !session.commands.is_closed() || session.active_turn.is_some())
        {
            return;
        }
        let (commands, rx) = async_channel::unbounded();
        let (cancel, cancel_rx) = async_channel::unbounded();
        let generation = Uuid::new_v4();
        let binding_id = config.binding_id.clone();
        self.sessions.insert(
            id,
            DesktopSession {
                generation,
                binding_id,
                session_id: config.session_id.clone(),
                models: Vec::new(),
                model_id: config.model_id.clone(),
                reasoning_efforts: Vec::new(),
                reasoning_effort: config.reasoning_effort.clone(),
                busy: false,
                ready: false,
                status: "Connecting to OpenCode…".to_owned(),
                error: None,
                approvals: Vec::new(),
                commands,
                cancel,
                active_turn: None,
                task: None,
                #[cfg(windows)]
                controller: controller.clone(),
                #[cfg(windows)]
                connecting: true,
                #[cfg(windows)]
                reload_pending: false,
            },
        );
        let foreground = ctx.spawner();
        let background = ctx.background_executor();
        let terminal_id = self.terminal_id;
        let error_controller = controller.clone();
        let task = ctx.spawn(
            async move {
                run(
                    id,
                    config,
                    controller,
                    foreground,
                    background,
                    terminal_id,
                    rx,
                    cancel_rx,
                )
                .await
            },
            move |me, result, ctx| {
                let Some(session) = me.sessions.get_mut(&id) else {
                    return;
                };
                if session.generation != generation {
                    return;
                }
                session.busy = false;
                #[cfg(windows)]
                {
                    session.connecting = false;
                }
                session.ready = false;
                session.approvals.clear();
                if let Err(error) = result {
                    let message = format!("{error:#}");
                    session.error = Some(message.clone());
                    session.status = "Disconnected".to_owned();
                    if let Some(turn) = session.active_turn.take() {
                        let controller = error_controller.clone();
                        ctx.spawn(
                            async move {
                                finish(
                                    &controller,
                                    &turn,
                                    StreamFinished {
                                        reason: Some(stream_finished::Reason::InternalError(
                                            stream_finished::InternalError { message },
                                        )),
                                        ..Default::default()
                                    },
                                )
                                .await
                            },
                            |_, _, _| {},
                        );
                    }
                }
                #[cfg(windows)]
                me.reload_configuration(ctx);
                ctx.emit(());
            },
        );
        self.sessions.get_mut(&id).unwrap().task = Some(task);
        ctx.emit(());
    }

    pub(crate) fn submit(
        &mut self,
        id: AIConversationId,
        turn: ExternalHarnessTurn,
        root: String,
        text: String,
        cwd: Option<String>,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        if session.busy {
            session.error = Some(
                "The agent is busy. Stop the current turn before sending another message."
                    .to_owned(),
            );
            ctx.emit(());
            return false;
        }
        session.busy = true;
        session.error = None;
        session.status = "Working…".to_owned();
        session.active_turn = Some(turn.clone());
        let sent = session
            .commands
            .try_send(Command::Prompt {
                turn,
                root,
                text,
                cwd,
            })
            .is_ok();
        ctx.emit(());
        sent
    }

    pub(crate) fn cancel(&mut self, id: AIConversationId, ctx: &mut ModelContext<Self>) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        if !session.busy || session.active_turn.is_none() {
            return false;
        }
        session.approvals.clear();
        session.status = "Stopping…".to_owned();
        let _ = session.cancel.try_send(());
        ctx.emit(());
        true
    }

    pub(crate) fn select_model(
        &mut self,
        id: AIConversationId,
        model: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.busy || session.commands.is_closed() {
            return;
        }
        if !session.models.iter().any(|choice| choice.id == model) {
            session.ready = false;
            session.status = "Choose an available model".to_owned();
            session.error = Some(format!(
                "The profile model '{model}' is unavailable. Connect its provider or choose another model; no fallback was used."
            ));
            session.model_id = Some(model.clone());
            let history = BlocklistAIHistoryModel::handle(ctx);
            if let Some(mut config) = history
                .as_ref(ctx)
                .conversation(&id)
                .and_then(|conversation| conversation.provider_session().cloned())
            {
                config.model_id = Some(model);
                config.reasoning_effort = None;
                history.update(ctx, |history, ctx| {
                    history.set_provider_session(id, Some(config), ctx)
                });
            }
            ctx.emit(());
            return;
        }
        session.busy = true;
        session.status = "Changing model…".to_owned();
        let _ = session.commands.try_send(Command::Model(model));
        ctx.emit(());
    }

    pub(crate) fn select_reasoning_effort(
        &mut self,
        id: AIConversationId,
        effort: String,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&id) else {
            return;
        };
        if session.busy
            || session.commands.is_closed()
            || !session
                .reasoning_efforts
                .iter()
                .any(|choice| choice.id == effort)
        {
            return;
        }
        session.busy = true;
        session.status = "Changing reasoning effort…".to_owned();
        let _ = session.commands.try_send(Command::ReasoningEffort(effort));
        ctx.emit(());
    }

    pub(crate) fn answer(
        &mut self,
        id: AIConversationId,
        approval: Uuid,
        allow: bool,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&id)
            && let Some(index) = session
                .approvals
                .iter()
                .position(|item| item.id == approval)
        {
            let request = session.approvals.remove(index);
            let _ = request.reply.send(allow);
        }
        ctx.emit(());
    }
}

enum Command {
    Prompt {
        turn: ExternalHarnessTurn,
        root: String,
        text: String,
        cwd: Option<String>,
    },
    Model(String),
    ReasoningEffort(String),
}

pub(crate) fn opening_actions(
    root: String,
    request: String,
    first: bool,
    prompt: &str,
) -> Vec<ClientAction> {
    let mapper = AcpTurnMapper::new(root, request, HashMap::new());
    if first {
        mapper.initial_actions(prompt)
    } else {
        vec![mapper.user_query_action(prompt, None)]
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    id: AIConversationId,
    mut config: ProviderChatSession,
    controller: ModelSpawner<BlocklistAIController>,
    foreground: ModelSpawner<DesktopAcp>,
    background: Arc<Background>,
    terminal_id: EntityId,
    commands: async_channel::Receiver<Command>,
    cancels: async_channel::Receiver<()>,
) -> Result<()> {
    if config.runtime == "unsupported_remote_opencode" {
        bail!(
            "Native OpenCode chat currently supports local host folders. Open a local tab, or use the local agent terminal launcher for SSH/WSL."
        );
    }
    if config.runtime != "opencode" {
        bail!(
            "This conversation requires unsupported runtime '{}'; it will not fall back to Warp.",
            config.runtime
        );
    }
    let cwd = PathBuf::from(&config.cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        bail!(
            "The conversation's working folder is unavailable: {}",
            config.cwd
        );
    }
    #[cfg(windows)]
    let runtime = warp_core::provider_setup::runtime_executable();
    #[cfg(not(windows))]
    let runtime = PathBuf::from("opencode");
    let mut process = command::r#async::Command::new(runtime);
    #[cfg(windows)]
    process.creation_flags(0);
    #[cfg(windows)]
    let profile = if warp_core::mywarp::enabled() {
        Some(warp_core::provider_setup::profile_dir()?)
    } else {
        warp_core::windows_package::profile_dir().map(PathBuf::from)
    };
    #[cfg(windows)]
    if let Some(profile) = &profile {
        warp_core::windows_package::prepare_opencode_profile(profile)?;
        if let Err(error) = warp_core::windows_package::import_existing_opencode_profile(profile) {
            log::warn!("Provider import unavailable; using the existing profile: {error}");
        }
        process.envs(warp_core::windows_package::opencode_environment(profile));
    }
    process
        .arg("acp")
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let inline_config = std::env::var("OPENCODE_CONFIG_CONTENT").ok();
    #[cfg(windows)]
    let inline_config = if profile.is_some() {
        None
    } else {
        inline_config
    };
    let mut attended: Value = serde_json::from_str(&attended_config(inline_config.as_deref())?)?;
    #[cfg(windows)]
    if let Some(profile) = &profile {
        let overrides = warp_core::provider_setup::runtime_overrides(profile)?;
        attended
            .as_object_mut()
            .context("Invalid runtime configuration")?
            .extend(
                overrides
                    .as_object()
                    .context("Invalid agent preferences")?
                    .clone(),
            );
    }
    if warp_core::mywarp::enabled() {
        require_profile_approval(&mut attended["permission"]);
    }
    process.env("OPENCODE_CONFIG_CONTENT", serde_json::to_string(&attended)?);
    let mut child = process.spawn().context("Could not start the AI runtime. Open Settings → AI providers and agent to check the provider connection.")?;
    #[cfg(target_os = "windows")]
    let runtime_job = {
        use std::os::windows::io::AsRawHandle as _;
        command::windows::JobObject::new()
            .kill_children_on_close()
            .assign_process(child.as_raw_handle() as isize)
            .create_scoped()
            .context("Could not manage the OpenCode process lifetime")?
    };
    let reader = child
        .stdout
        .take()
        .context("OpenCode did not provide stdout")?;
    let writer = child
        .stdin
        .take()
        .context("OpenCode did not provide stdin")?;
    if let Some(mut stderr) = child.stderr.take() {
        background
            .spawn(async move {
                let _ = futures::io::copy(&mut stderr, &mut futures::io::sink()).await;
            })
            .detach();
    }
    let handler = permission_handler(
        id,
        config.binding_id.clone(),
        terminal_id,
        foreground.clone(),
        controller.clone(),
    );
    let (connection, notifications) = AcpConnection::new(reader, writer, handler, &background);
    let connection = Arc::new(connection);
    let settings = {
        let setup = initialize(&connection, &config).fuse();
        let timeout = Timer::after(Duration::from_secs(60)).fuse();
        let stopped = cancels.recv().fuse();
        pin_mut!(setup, timeout, stopped);
        select_biased! {
            _ = stopped => { bail!("Connection cancelled. Retry when you are ready."); },
            result = setup => result?,
            _ = timeout => { bail!("OpenCode did not finish connecting within 60 seconds."); },
        }
    };
    config.session_id = Some(settings.0);
    let mut options = settings.1;
    while notifications.try_recv().is_ok() {}
    let mut selection = if let Some(model) = config.model_id.clone() {
        if model_choices(&options)
            .1
            .iter()
            .any(|choice| choice.id == model)
        {
            options = set_model(
                &connection,
                config.session_id.as_deref().unwrap(),
                &options,
                &model,
            )
            .await?;
            Ok(())
        } else {
            Err(anyhow!(
                "The saved model '{model}' is unavailable. Choose an available model; no fallback was used."
            ))
        }
    } else {
        Ok(())
    };
    if selection.is_ok()
        && let Some(effort) = config.reasoning_effort.as_deref()
    {
        selection = set_reasoning_effort(
            &connection,
            config.session_id.as_deref().unwrap(),
            &options,
            effort,
        )
        .await
        .map(|updated| {
            options = updated;
        });
    }
    match selection {
        Ok(()) => publish_settings(id, &mut config, &options, &foreground, &controller).await?,
        Err(error) => {
            let (_, choices, _) = model_choices(&options);
            let (_, reasoning_efforts, reasoning_effort) =
                setting_choices(&options, "thought_level");
            let message = format!("{error:#}");
            foreground
                .spawn(move |me, ctx| {
                    if let Some(session) = me.sessions.get_mut(&id) {
                        session.models = choices;
                        session.reasoning_efforts = reasoning_efforts;
                        session.reasoning_effort = reasoning_effort;
                        #[cfg(windows)]
                        {
                            session.connecting = false;
                        }
                        session.status = "Check model and reasoning effort".to_owned();
                        session.error = Some(message);
                    }
                    #[cfg(windows)]
                    me.reload_configuration(ctx);
                    ctx.emit(());
                })
                .await?;
        }
    }
    loop {
        let command = commands.recv().await;
        let Ok(command) = command else {
            break;
        };
        match command {
            Command::Model(model) => {
                match set_model(
                    &connection,
                    config.session_id.as_deref().unwrap(),
                    &options,
                    &model,
                )
                .await
                {
                    Ok(updated) => {
                        options = updated;
                        publish_settings(id, &mut config, &options, &foreground, &controller)
                            .await?;
                    }
                    Err(error) => {
                        return Err(
                            error.context("Model selection failed; reconnect before continuing")
                        );
                    }
                }
            }
            Command::ReasoningEffort(effort) => {
                options = set_reasoning_effort(
                    &connection,
                    config.session_id.as_deref().unwrap(),
                    &options,
                    &effort,
                )
                .await
                .context("Reasoning effort selection failed; reconnect before continuing")?;
                publish_settings(id, &mut config, &options, &foreground, &controller).await?;
            }
            Command::Prompt {
                mut turn,
                root,
                text,
                cwd,
            } => {
                let mut mapper = AcpTurnMapper::new(root, turn.request_id.clone(), HashMap::new());
                let result = prompt(
                    id,
                    &connection,
                    &notifications,
                    &cancels,
                    &config,
                    &mut mapper,
                    &mut turn,
                    &controller,
                    &foreground,
                    text,
                    cwd,
                )
                .await;
                match result {
                    Ok(StopReason::Cancelled) => {
                        let stream = turn.stream_id.clone();
                        controller
                            .spawn(move |controller, ctx| {
                                controller.cancel_external_harness_turn(
                                    &stream,
                                    CancellationReason::ManuallyCancelled,
                                    ctx,
                                )
                            })
                            .await?;
                    }
                    Ok(reason) => finish(&controller, &turn, stream_finished_for(reason)).await?,
                    Err(error) => return Err(error),
                }
                while cancels.try_recv().is_ok() {}
                foreground
                    .spawn(move |me, ctx| {
                        if let Some(session) = me.sessions.get_mut(&id) {
                            session.busy = false;
                            session.active_turn = None;
                            session.approvals.clear();
                            session.status = "Ready".to_owned();
                        }
                        #[cfg(windows)]
                        me.reload_configuration(ctx);
                        ctx.emit(());
                    })
                    .await?;
            }
        }
    }
    connection.close().await;
    drop(child);
    #[cfg(target_os = "windows")]
    drop(runtime_job);
    Ok(())
}

fn attended_config(existing: Option<&str>) -> Result<String> {
    let mut config: Value = match existing {
        Some(text) => {
            serde_json::from_str(text).context("OPENCODE_CONFIG_CONTENT is invalid JSON")?
        }
        None => json!({}),
    };
    let object = config
        .as_object_mut()
        .context("OpenCode inline config must be an object")?;
    object.insert("permission".to_owned(), json!("ask"));
    Ok(serde_json::to_string(&config)?)
}

fn require_profile_approval(permission: &mut Value) {
    match permission {
        Value::String(value) if value == "allow" => *value = "ask".to_owned(),
        Value::Object(values) => {
            for value in values.values_mut() {
                require_profile_approval(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {}
    }
}

async fn initialize(
    connection: &AcpConnection,
    config: &ProviderChatSession,
) -> Result<(String, Value)> {
    let response: InitializeResponse = connection.request(protocol::METHOD_INITIALIZE, json!({ "protocolVersion": 1, "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false }, "clientInfo": { "name": "mywarp", "title": "MyWarp", "version": env!("CARGO_PKG_VERSION") } })).await?;
    if response.protocol_version != 1 {
        bail!("OpenCode returned an incompatible ACP protocol version.");
    }
    if let Some(session_id) = &config.session_id {
        if !response.agent_capabilities.load_session {
            bail!(
                "This runtime cannot reload the saved session. Start a new chat; this conversation will not be replaced."
            );
        }
        let response: Value = connection
            .request(
                "session/load",
                json!({"sessionId":session_id,"cwd":config.cwd,"mcpServers":[]}),
            )
            .await
            .context("Could not resume the saved OpenCode session")?;
        Ok((session_id.clone(), response["configOptions"].clone()))
    } else {
        let response: Value = connection
            .request(
                protocol::METHOD_SESSION_NEW,
                json!({"cwd":config.cwd,"mcpServers":[]}),
            )
            .await
            .context("Could not create an OpenCode session")?;
        let session_id = response["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("OpenCode returned no session ID")?
            .to_owned();
        Ok((session_id, response["configOptions"].clone()))
    }
}

fn setting_choices(
    options: &Value,
    category: &str,
) -> (Option<String>, Vec<ModelChoice>, Option<String>) {
    fn collect(options: &Value, result: &mut Vec<ModelChoice>) {
        if let Some(options) = options.as_array() {
            for option in options {
                if let Some(value) = option["value"].as_str() {
                    result.push(ModelChoice {
                        id: value.to_owned(),
                        label: option["name"].as_str().unwrap_or(value).to_owned(),
                    });
                } else {
                    collect(&option["options"], result);
                }
            }
        }
    }
    let setting = options.as_array().and_then(|options| {
        options
            .iter()
            .find(|setting| setting["category"] == category && setting["type"] == "select")
    });
    let Some(setting) = setting else {
        return (None, Vec::new(), None);
    };
    let mut choices = Vec::new();
    collect(&setting["options"], &mut choices);
    (
        setting["id"].as_str().map(str::to_owned),
        choices,
        setting["currentValue"].as_str().map(str::to_owned),
    )
}

fn model_choices(options: &Value) -> (Option<String>, Vec<ModelChoice>, Option<String>) {
    let (id, mut choices, selected) = setting_choices(options, "model");
    for choice in &mut choices {
        choice.label = format!("{} ({})", choice.label, choice.id);
    }
    (id, choices, selected)
}

async fn set_reasoning_effort(
    connection: &AcpConnection,
    session: &str,
    options: &Value,
    effort: &str,
) -> Result<Value> {
    let (config_id, choices, _) = setting_choices(options, "thought_level");
    if !choices.iter().any(|choice| choice.id == effort) {
        bail!(
            "OpenCode does not offer reasoning effort '{effort}' for this model. Choose a supported effort; no fallback was used."
        );
    }
    let updated = set_config_option(
        connection,
        session,
        config_id
            .as_deref()
            .context("OpenCode did not advertise a reasoning effort setting")?,
        effort,
    )
    .await?;
    if setting_choices(&updated, "thought_level").2.as_deref() != Some(effort)
        || model_choices(&updated).2 != model_choices(options).2
    {
        bail!(
            "OpenCode did not confirm the requested reasoning effort and model; no prompt was sent."
        );
    }
    Ok(updated)
}

async fn set_model(
    connection: &AcpConnection,
    session: &str,
    options: &Value,
    model: &str,
) -> Result<Value> {
    let (config_id, choices, _) = model_choices(options);
    if !choices.iter().any(|choice| choice.id == model) {
        bail!(
            "OpenCode does not offer the saved model '{model}'. Choose an available model; no fallback was used."
        );
    }
    let updated = set_config_option(
        connection,
        session,
        config_id
            .as_deref()
            .context("OpenCode did not advertise a model setting")?,
        model,
    )
    .await?;
    if model_choices(&updated).2.as_deref() != Some(model) {
        bail!("OpenCode did not confirm the requested model; no prompt was sent.");
    }
    Ok(updated)
}

async fn set_config_option(
    connection: &AcpConnection,
    session: &str,
    config_id: &str,
    value: &str,
) -> Result<Value> {
    let request = connection.request::<_, Value>(
        protocol::METHOD_SESSION_SET_CONFIG_OPTION,
        json!({"sessionId":session,"configId":config_id,"value":value}),
    );
    let response = match select(
        Box::pin(request),
        Box::pin(Timer::after(Duration::from_secs(30))),
    )
    .await
    {
        Either::Left((result, _)) => result?,
        Either::Right(_) => bail!("OpenCode did not confirm the setting change within 30 seconds."),
    };
    Ok(response["configOptions"].clone())
}

async fn publish_settings(
    id: AIConversationId,
    config: &mut ProviderChatSession,
    options: &Value,
    foreground: &ModelSpawner<DesktopAcp>,
    controller: &ModelSpawner<BlocklistAIController>,
) -> Result<()> {
    let (_, choices, current) = model_choices(options);
    let (_, reasoning_efforts, reasoning_effort) = setting_choices(options, "thought_level");
    config.model_id = current.or_else(|| config.model_id.clone());
    config.reasoning_effort = reasoning_effort.clone();
    let saved = config.clone();
    controller
        .spawn(move |_, ctx| {
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                if history
                    .conversation(&id)
                    .and_then(|conversation| conversation.provider_session())
                    .is_some_and(|current| current.binding_id == saved.binding_id)
                {
                    history.set_provider_session(id, Some(saved), ctx);
                }
            })
        })
        .await?;
    let selected = config.model_id.clone();
    let binding = config.binding_id.clone();
    let session_id = config.session_id.clone();
    foreground
        .spawn(move |me, ctx| {
            if let Some(session) = me.sessions.get_mut(&id)
                && session.binding_id == binding
            {
                #[cfg(windows)]
                {
                    session.connecting = false;
                }
                session.ready = selected
                    .as_ref()
                    .is_some_and(|selected| choices.iter().any(|choice| choice.id == *selected));
                session.models = choices;
                session.model_id = selected;
                session.reasoning_efforts = reasoning_efforts;
                session.reasoning_effort = reasoning_effort;
                session.session_id = session_id;
                session.error = None;
                session.busy = session.active_turn.is_some();
                session.status = if session.busy {
                    "Working…"
                } else if session.ready {
                    "Ready"
                } else {
                    "Choose a model"
                }
                .to_owned();
            }
            #[cfg(windows)]
            me.reload_configuration(ctx);
            ctx.emit(());
        })
        .await?;
    Ok(())
}

fn permission_handler(
    id: AIConversationId,
    binding_id: String,
    terminal_id: EntityId,
    foreground: ModelSpawner<DesktopAcp>,
    controller: ModelSpawner<BlocklistAIController>,
) -> AgentRequestHandler {
    Arc::new(move |method, value| {
        let foreground = foreground.clone();
        let controller = controller.clone();
        let binding_id = binding_id.clone();
        if method != protocol::METHOD_REQUEST_PERMISSION {
            return futures::future::ready(Err(RpcError::method_not_found(method))).boxed();
        }
        async move {
            let params: RequestPermissionParams =
                serde_json::from_value(value.clone()).map_err(RpcError::internal)?;
            let allow = params
                .options
                .iter()
                .find(|option| option.kind == PermissionOptionKind::AllowOnce)
                .map(|option| option.option_id.clone());
            let reject = params
                .options
                .iter()
                .find(|option| option.kind == PermissionOptionKind::RejectOnce)
                .map(|option| option.option_id.clone());
            let requested_session = value["sessionId"].as_str().map(str::to_owned);
            let checked_binding = binding_id.clone();
            let checked_session = requested_session.clone();
            let active = foreground
                .spawn(move |me, _| {
                    me.sessions.get(&id).is_some_and(|session| {
                        session.binding_id == checked_binding
                            && session.session_id == checked_session
                            && checked_session.is_some()
                            && session.active_turn.is_some()
                            && session.status != "Stopping…"
                    })
                })
                .await
                .map_err(RpcError::internal)?;
            if !active {
                return serde_json::to_value(RequestPermissionResponse {
                    outcome: PermissionOutcome::Cancelled,
                })
                .map_err(RpcError::internal);
            }
            let requested_binding = binding_id.clone();
            let folder = controller
                .spawn(move |_, ctx| {
                    BlocklistAIHistoryModel::as_ref(ctx)
                        .conversation(&id)
                        .and_then(|conversation| conversation.provider_session())
                        .filter(|config| config.binding_id == requested_binding)
                        .map(|config| PathBuf::from(&config.cwd))
                })
                .await
                .map_err(RpcError::internal)?;
            let request =
                PolicyRequest::for_tool_call(&params.tool_call).map(|request| match request {
                    PolicyRequest::WriteFiles { paths } => PolicyRequest::WriteFiles {
                        paths: paths
                            .into_iter()
                            .map(|path| {
                                if path.is_absolute() {
                                    path
                                } else {
                                    folder.as_ref().map(|cwd| cwd.join(&path)).unwrap_or(path)
                                }
                            })
                            .collect(),
                    },
                    request => request,
                });
            let profile_call = params.tool_call.clone();
            let denied = controller
                .spawn(move |controller, ctx| {
                    let decision = request.map(|request| {
                        request.evaluate_for_conversation(
                            controller,
                            id,
                            terminal_id,
                            controller.provider_escape_char(ctx),
                            ctx,
                        )
                    });
                    let autoapprove = warp_core::mywarp::enabled()
                        && (tool_is_read_only(&profile_call, controller.provider_escape_char(ctx))
                            || profile_allows_tool(
                                controller,
                                id,
                                terminal_id,
                                &profile_call,
                                folder.as_deref(),
                                ctx,
                            ));
                    (decision, autoapprove)
                })
                .await
                .map_err(RpcError::internal)?;
            if let Some(PolicyDecision::Deny { reason }) = denied.0 {
                foreground
                    .spawn(move |me, ctx| {
                        me.set_error(id, format!("Action denied by policy: {reason}"), ctx)
                    })
                    .await
                    .map_err(RpcError::internal)?;
                return serde_json::to_value(RequestPermissionResponse {
                    outcome: reject
                        .map(|option_id| PermissionOutcome::Selected { option_id })
                        .unwrap_or(PermissionOutcome::Cancelled),
                })
                .map_err(RpcError::internal);
            }
            if denied.1
                && let Some(option_id) = allow.clone()
            {
                return serde_json::to_value(RequestPermissionResponse {
                    outcome: PermissionOutcome::Selected { option_id },
                })
                .map_err(RpcError::internal);
            }
            let (reply, answer) = oneshot::channel();
            let can_allow = allow.is_some();
            let approval = Uuid::new_v4();
            let kind = effective_tool_kind(&params.tool_call);
            let title = params
                .tool_call
                .title
                .unwrap_or_else(|| "Agent requests permission".to_owned());
            let details =
                serde_json::to_string_pretty(&value["toolCall"]).map_err(RpcError::internal)?;
            foreground
                .spawn(move |me, ctx| {
                    if let Some(session) = me.sessions.get_mut(&id)
                        && session.binding_id == binding_id
                        && session.session_id == requested_session
                        && requested_session.is_some()
                        && session.active_turn.is_some()
                        && session.status != "Stopping…"
                    {
                        session.approvals.push(Approval {
                            id: approval,
                            title,
                            details,
                            can_allow,
                            kind,
                            reply,
                        });
                    }
                    ctx.emit(());
                })
                .await
                .map_err(RpcError::internal)?;
            let approved = answer.await.unwrap_or(false);
            let choice = if approved { allow } else { reject };
            let outcome = choice
                .map(|option_id| PermissionOutcome::Selected { option_id })
                .unwrap_or(PermissionOutcome::Cancelled);
            serde_json::to_value(RequestPermissionResponse { outcome }).map_err(RpcError::internal)
        }
        .boxed()
    })
}

fn tool_is_read_only(
    call: &protocol::ToolCallFields,
    escape_char: warp_util::path::EscapeChar,
) -> bool {
    let kind = effective_tool_kind(call);
    match kind {
        Some(protocol::ToolKind::Read | protocol::ToolKind::Search | protocol::ToolKind::Think) => {
            true
        }
        Some(protocol::ToolKind::Execute) => {
            super::mapping::command_from_raw_input(call.raw_input.as_ref(), kind, "")
                .is_some_and(|command| super::read_only::is_read_only(&command, escape_char))
        }
        Some(
            protocol::ToolKind::Edit
            | protocol::ToolKind::Delete
            | protocol::ToolKind::Move
            | protocol::ToolKind::Fetch
            | protocol::ToolKind::SwitchMode
            | protocol::ToolKind::Other
            | protocol::ToolKind::Unknown,
        )
        | None => false,
    }
}

fn profile_allows_tool(
    controller: &BlocklistAIController,
    id: AIConversationId,
    terminal_id: EntityId,
    call: &protocol::ToolCallFields,
    folder: Option<&std::path::Path>,
    ctx: &warpui::AppContext,
) -> bool {
    let permissions = BlocklistAIPermissions::as_ref(ctx);
    let scope = controller.team_context(ctx);
    let paths = || {
        let mut paths: Vec<_> = call
            .locations
            .iter()
            .flatten()
            .map(|location| PathBuf::from(&location.path))
            .collect();
        if let Some(path) = super::mapping::path_from_raw_input(call.raw_input.as_ref()) {
            paths.push(PathBuf::from(path));
        }
        if let Some((path, _, _)) = super::mapping::first_diff(call.content.as_deref()) {
            paths.push(PathBuf::from(path));
        }
        paths
            .into_iter()
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    folder.map(|folder| folder.join(&path)).unwrap_or(path)
                }
            })
            .collect::<Vec<_>>()
    };
    let kind = effective_tool_kind(call);
    match kind {
        Some(protocol::ToolKind::Execute) => super::mapping::command_from_raw_input(
            call.raw_input.as_ref(),
            kind,
            call.title.as_deref().unwrap_or_default(),
        )
        .is_some_and(|command| {
            permissions
                .can_autoexecute_command(
                    &id,
                    &command,
                    controller.provider_escape_char(ctx),
                    false,
                    None,
                    Some(terminal_id),
                    &scope,
                    ctx,
                )
                .is_allowed()
        }),
        Some(protocol::ToolKind::Edit | protocol::ToolKind::Delete | protocol::ToolKind::Move) => {
            let paths = paths();
            !paths.is_empty()
                && permissions
                    .can_write_files(&id, &paths, Some(terminal_id), &scope, ctx)
                    .is_allowed()
        }
        Some(protocol::ToolKind::Read | protocol::ToolKind::Search) => {
            let paths = paths();
            !paths.is_empty()
                && permissions
                    .can_read_files_with_conversation(&id, paths, Some(terminal_id), &scope, ctx)
                    .is_allowed()
        }
        Some(
            protocol::ToolKind::Think
            | protocol::ToolKind::Fetch
            | protocol::ToolKind::SwitchMode
            | protocol::ToolKind::Other
            | protocol::ToolKind::Unknown,
        )
        | None => false,
    }
}

fn prompt_content(text: String, cwd: &str) -> Result<Vec<ContentBlock>> {
    Ok(vec![
        ContentBlock::Text {
            text: format!(
                "<terminal_context>\nCurrent working directory: {}\nFor shell commands, explicitly set the workdir parameter to this directory. Resolve relative file paths against this directory and pass absolute paths to file tools. This directory supersedes directories from earlier messages or the session's initial working directory.\n</terminal_context>",
                serde_json::to_string(cwd)?,
            ),
        },
        ContentBlock::Text { text },
    ])
}

#[allow(clippy::too_many_arguments)]
async fn prompt(
    id: AIConversationId,
    connection: &Arc<AcpConnection>,
    notifications: &async_channel::Receiver<InboundNotification>,
    cancels: &async_channel::Receiver<()>,
    config: &ProviderChatSession,
    mapper: &mut AcpTurnMapper,
    turn: &mut ExternalHarnessTurn,
    controller: &ModelSpawner<BlocklistAIController>,
    foreground: &ModelSpawner<DesktopAcp>,
    text: String,
    cwd: Option<String>,
) -> Result<StopReason> {
    let session_id = config
        .session_id
        .as_deref()
        .context("No OpenCode session")?;
    let response = connection
        .send_request::<_, PromptResponse>(
            protocol::METHOD_SESSION_PROMPT,
            PromptParams {
                session_id: session_id.to_owned(),
                prompt: prompt_content(text, cwd.as_deref().unwrap_or(&config.cwd))?,
            },
        )
        .await?
        .fuse();
    pin_mut!(response);
    loop {
        let update = notifications.recv().fuse();
        let cancel = cancels.recv().fuse();
        pin_mut!(update, cancel);
        select_biased! {
            _ = cancel => {
                connection.cancel_pending_permission_requests(session_id).await;
                connection.notify(protocol::METHOD_SESSION_CANCEL, json!({"sessionId":session_id})).await?;
                let stopped = select(response.as_mut(), Box::pin(Timer::after(Duration::from_secs(15)))).await;
                return match stopped { Either::Left(_) => Ok(StopReason::Cancelled), Either::Right(_) => Err(anyhow!("OpenCode did not stop within 15 seconds; the connection was closed.")) };
            }
            update = update => {
                let update = update.context("OpenCode disconnected")?;
                apply_notification(id, session_id, update, mapper, turn, controller, foreground).await?;
            }
            result = response => {
                while let Ok(update) = notifications.try_recv() { apply_notification(id, session_id, update, mapper, turn, controller, foreground).await?; }
                apply_events(id, mapper.finish_events(), mapper, turn, controller, foreground).await?;
                return Ok(result?.stop_reason);
            }
        }
    }
}

async fn apply_notification(
    id: AIConversationId,
    session_id: &str,
    notification: InboundNotification,
    mapper: &mut AcpTurnMapper,
    turn: &mut ExternalHarnessTurn,
    controller: &ModelSpawner<BlocklistAIController>,
    foreground: &ModelSpawner<DesktopAcp>,
) -> Result<()> {
    if notification.method != protocol::METHOD_SESSION_UPDATE {
        return Ok(());
    }
    if notification.params["sessionId"].as_str() != Some(session_id) {
        return Ok(());
    }
    let update: SessionUpdateParams = serde_json::from_value(notification.params)?;
    let events = mapper.map_update(update.update);
    apply_events(id, events, mapper, turn, controller, foreground).await
}

async fn apply_events(
    id: AIConversationId,
    events: Vec<TurnEvent>,
    mapper: &mut AcpTurnMapper,
    turn: &mut ExternalHarnessTurn,
    controller: &ModelSpawner<BlocklistAIController>,
    foreground: &ModelSpawner<DesktopAcp>,
) -> Result<()> {
    for event in events {
        match event {
            TurnEvent::Actions(actions) => {
                let stream = turn.stream_id.clone();
                controller
                    .spawn(move |controller, ctx| {
                        controller.apply_external_harness_event(
                            &stream,
                            ResponseEvent {
                                r#type: Some(response_event::Type::ClientActions(
                                    response_event::ClientActions { actions },
                                )),
                            },
                            ctx,
                        )
                    })
                    .await?;
            }
            TurnEvent::SegmentBoundary => {
                finish(controller, turn, stream_finished_for(StopReason::EndTurn)).await?;
                *turn = controller
                    .spawn(move |controller, ctx| {
                        controller.begin_external_harness_turn(id, None, ctx)
                    })
                    .await??;
                mapper.start_segment(turn.request_id.clone());
                let current = turn.clone();
                foreground
                    .spawn(move |me, ctx| {
                        if let Some(session) = me.sessions.get_mut(&id) {
                            session.active_turn = Some(current);
                        }
                        ctx.emit(());
                    })
                    .await?;
            }
        }
    }
    Ok(())
}

async fn finish(
    controller: &ModelSpawner<BlocklistAIController>,
    turn: &ExternalHarnessTurn,
    finished: StreamFinished,
) -> Result<()> {
    let stream = turn.stream_id.clone();
    controller
        .spawn(move |controller, ctx| {
            controller.apply_external_harness_event(
                &stream,
                ResponseEvent {
                    r#type: Some(response_event::Type::Finished(finished)),
                },
                ctx,
            )
        })
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
