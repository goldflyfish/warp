//! Provider settings use the same profile and runtime as the native chat footer.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context as _, bail};
use instant::Instant;
use serde_json::{Value, json};
use warp_core::provider_setup::{CONNECTION_CHECK_TIMEOUT, ProviderServer, Snapshot, offload};
use warpui::elements::new_scrollable::SingleAxisConfig;
use warpui::elements::{
    ClippedScrollStateHandle, NewScrollable, SavePosition, ScrollTarget, ScrollToPositionMode,
};
use warpui::platform::Cursor;
use warpui::units::Pixels;

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderSetupAction {
    SelectProvider(String),
    SelectModel(String),
    Refresh,
    OpenConfiguration,
    OpenApiKeyFile,
    Save,
    Disconnect,
    Test,
    CancelTest,
    DefaultModel,
    Authorize(usize),
    FinishAuthorization,
    CancelAuthorization,
    AgentMode(String),
    ToolPolicy(String),
    SaveInstructions,
    AddMcp,
    RemoveMcp(String),
    McpType(String),
    EditMcp(String),
    ToggleMcp(String),
    ShowProfileEditor(bool),
    EditProfile,
    SaveProfile,
    RemoveProfile,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FeedbackScope {
    Connection,
    Models,
    Agent,
    Policy,
    Instructions,
    Mcp,
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderMarker {
    Default,
    Verified,
    Untested,
}

fn provider_marker(snapshot: &Snapshot, id: &str) -> ProviderMarker {
    if snapshot
        .default_model
        .split_once('/')
        .is_some_and(|(provider, _)| provider == id)
    {
        ProviderMarker::Default
    } else if snapshot.verified.iter().any(|(provider, _)| provider == id) {
        ProviderMarker::Verified
    } else {
        ProviderMarker::Untested
    }
}

fn feedback_scope(action: &ProviderSetupAction) -> FeedbackScope {
    match action {
        ProviderSetupAction::SelectModel(_)
        | ProviderSetupAction::Test
        | ProviderSetupAction::CancelTest
        | ProviderSetupAction::DefaultModel => FeedbackScope::Models,
        ProviderSetupAction::AgentMode(_)
        | ProviderSetupAction::ShowProfileEditor(_)
        | ProviderSetupAction::EditProfile
        | ProviderSetupAction::SaveProfile
        | ProviderSetupAction::RemoveProfile => FeedbackScope::Agent,
        ProviderSetupAction::ToolPolicy(_) => FeedbackScope::Policy,
        ProviderSetupAction::SaveInstructions => FeedbackScope::Instructions,
        ProviderSetupAction::AddMcp
        | ProviderSetupAction::RemoveMcp(_)
        | ProviderSetupAction::McpType(_)
        | ProviderSetupAction::EditMcp(_)
        | ProviderSetupAction::ToggleMcp(_) => FeedbackScope::Mcp,
        _ => FeedbackScope::Connection,
    }
}

pub(super) struct ProviderSetup {
    server: Option<Arc<ProviderServer>>,
    reload_pending: bool,
    configuration_generation: u64,
    loaded_endpoint: String,
    loaded_custom_model: String,
    loaded_instructions: String,
    mcp_type: String,
    show_profile_editor: bool,
    mcp_type_dropdown: ViewHandle<Dropdown<WarpAgentPageAction>>,
    mcp_arguments: ViewHandle<EditorView>,
    profile_name: ViewHandle<EditorView>,
    profile_prompt: ViewHandle<EditorView>,
    snapshot: Snapshot,
    selected: String,
    model: String,
    status: String,
    feedback_scope: FeedbackScope,
    error: bool,
    busy: bool,
    authorization: Option<(String, usize, String, bool)>,
    provider_dropdown: ViewHandle<FilterableDropdown<WarpAgentPageAction>>,
    model_dropdown: ViewHandle<FilterableDropdown<WarpAgentPageAction>>,
    mode_dropdown: ViewHandle<Dropdown<WarpAgentPageAction>>,
    policy_dropdown: ViewHandle<Dropdown<WarpAgentPageAction>>,
    key: ViewHandle<EditorView>,
    endpoint: ViewHandle<EditorView>,
    custom_model: ViewHandle<EditorView>,
    code: ViewHandle<EditorView>,
    instructions: ViewHandle<EditorView>,
    mcp_name: ViewHandle<EditorView>,
    mcp_command: ViewHandle<EditorView>,
    provider_rows: BTreeMap<String, MouseStateHandle>,
    model_rows: BTreeMap<String, MouseStateHandle>,
    model_scroll: ClippedScrollStateHandle,
    connection_check: Option<(Instant, Arc<AtomicBool>)>,
    cancel_test: MouseStateHandle,
    buttons: [MouseStateHandle; 15],
    oauth_buttons: Vec<MouseStateHandle>,
    mcp_buttons: Vec<[MouseStateHandle; 3]>,
}

fn action(value: ProviderSetupAction) -> WarpAgentPageAction {
    WarpAgentPageAction::Provider(value)
}

fn editor(
    placeholder: &str,
    password: bool,
    multiline: bool,
    ctx: &mut ViewContext<WarpAgentPageView>,
) -> ViewHandle<EditorView> {
    let placeholder = placeholder.to_owned();
    ctx.add_typed_action_view(|ctx| {
        let appearance = Appearance::as_ref(ctx);
        let text = TextOptions {
            font_size_override: Some(appearance.ui_font_size()),
            font_family_override: Some(appearance.monospace_font_family()),
            ..Default::default()
        };
        let mut editor = if multiline {
            EditorView::new(
                EditorOptions {
                    text,
                    soft_wrap: true,
                    autogrow: true,
                    ..Default::default()
                },
                ctx,
            )
        } else {
            EditorView::single_line(
                SingleLineEditorOptions {
                    is_password: password,
                    text,
                    propagate_and_no_op_vertical_navigation_keys:
                        PropagateAndNoOpNavigationKeys::Always,
                    ..Default::default()
                },
                ctx,
            )
        };
        editor.set_placeholder_text(&placeholder, ctx);
        editor
    })
}

impl ProviderSetup {
    pub(super) fn new(ctx: &mut ViewContext<WarpAgentPageView>) -> Self {
        let provider_dropdown = ctx.add_typed_action_view(FilterableDropdown::new);
        let model_dropdown = ctx.add_typed_action_view(FilterableDropdown::new);
        provider_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_placeholder("+ Add provider", ctx);
            dropdown.set_top_bar_height(36., ctx);
            dropdown.set_top_bar_max_width(170.);
            dropdown.set_menu_width(340., ctx);
        });
        model_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_placeholder("Choose a model", ctx);
            dropdown.set_top_bar_height(36., ctx);
            dropdown.set_top_bar_max_width(700.);
        });
        let mode_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut d = Dropdown::new(ctx);
            d.add_items(
                vec![
                    DropdownItem::new(
                        "Build — edit files and run tools",
                        action(ProviderSetupAction::AgentMode("build".into())),
                    ),
                    DropdownItem::new(
                        "Plan — research before making changes",
                        action(ProviderSetupAction::AgentMode("plan".into())),
                    ),
                ],
                ctx,
            );
            d.set_top_bar_max_width(420.);
            d
        });
        let policy_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut d = Dropdown::new(ctx);
            d.add_items(
                vec![
                    DropdownItem::new(
                        "Ask before every tool",
                        action(ProviderSetupAction::ToolPolicy("ask".into())),
                    ),
                    DropdownItem::new(
                        "Allow reading; ask before changes",
                        action(ProviderSetupAction::ToolPolicy("review".into())),
                    ),
                    DropdownItem::new(
                        "Read only — deny commands and changes",
                        action(ProviderSetupAction::ToolPolicy("readonly".into())),
                    ),
                ],
                ctx,
            );
            d.set_top_bar_max_width(420.);
            d
        });
        let mcp_type_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut d = Dropdown::new(ctx);
            d.add_items(
                vec![
                    DropdownItem::new(
                        "Remote server",
                        action(ProviderSetupAction::McpType("remote".into())),
                    ),
                    DropdownItem::new(
                        "Local command",
                        action(ProviderSetupAction::McpType("local".into())),
                    ),
                ],
                ctx,
            );
            d.set_selected_by_action(action(ProviderSetupAction::McpType("remote".into())), ctx);
            d.set_top_bar_max_width(420.);
            d
        });
        let instructions = editor(
            "Tell the agent how you prefer it to work…",
            false,
            true,
            ctx,
        );
        if let Ok(profile) = warp_core::provider_setup::profile_dir()
            && let Ok(text) = std::fs::read_to_string(
                warp_core::windows_package::configuration_dir(&profile)
                    .join("agent-instructions.md"),
            )
        {
            instructions.update(ctx, |editor, ctx| editor.set_buffer_text(&text, ctx));
        }
        let loaded_instructions = instructions.as_ref(ctx).buffer_text(ctx);
        if ctx.has_singleton_model::<crate::configuration_watcher::ConfigurationWatcher>() {
            ctx.subscribe_to_model(
                &crate::configuration_watcher::ConfigurationWatcher::handle(ctx),
                |me, _, event, ctx| {
                    let Some(state) = me.provider_setup.as_mut() else {
                        return;
                    };
                    match event {
                        crate::configuration_watcher::ConfigurationEvent::Changed => {
                            state.configuration_generation += 1;
                            state.snapshot.verified.clear();
                            state.reload_pending = true;
                        }
                        crate::configuration_watcher::ConfigurationEvent::Invalid(error) => {
                            state.configuration_generation += 1;
                            state.snapshot.verified.clear();
                            if let Some((_, cancelled)) = &state.connection_check {
                                cancelled.store(true, Ordering::Relaxed);
                            }
                            state.reload_pending = false;
                            state.feedback_scope = FeedbackScope::Connection;
                            state.status = format!("Configuration was not reloaded: {error}");
                            state.error = true;
                            ctx.notify();
                            return;
                        }
                        crate::configuration_watcher::ConfigurationEvent::Checked => {}
                    }
                    me.refresh_provider_configuration(ctx);
                },
            );
        }
        let mut me = Self {
            mcp_type: "remote".into(),
            show_profile_editor: false,
            mcp_type_dropdown,
            mcp_arguments: editor("Each argument on its own line", false, true, ctx),
            profile_name: editor("Profile name, e.g. code-review", false, false, ctx),
            profile_prompt: editor(
                "Describe this agent's role and how it should work…",
                false,
                true,
                ctx,
            ),
            server: None,
            reload_pending: false,
            configuration_generation: 0,
            loaded_endpoint: String::new(),
            loaded_custom_model: String::new(),
            loaded_instructions,
            snapshot: Snapshot::default(),
            selected: "openrouter".into(),
            model: String::new(),
            status: "Loading your provider configuration…".into(),
            feedback_scope: FeedbackScope::Connection,
            error: false,
            busy: true,
            authorization: None,
            provider_dropdown,
            model_dropdown,
            mode_dropdown,
            policy_dropdown,
            key: editor("Enter API key", true, false, ctx),
            endpoint: editor(
                "Provider default, or https://your-endpoint/v1",
                false,
                false,
                ctx,
            ),
            custom_model: editor("Model ID returned by your endpoint", false, false, ctx),
            code: editor("Paste the authorization code", true, false, ctx),
            instructions,
            mcp_name: editor("Server name, e.g. filesystem", false, false, ctx),
            mcp_command: editor("https://your-server/mcp", false, false, ctx),
            provider_rows: Default::default(),
            model_rows: Default::default(),
            model_scroll: Default::default(),
            connection_check: None,
            cancel_test: Default::default(),
            buttons: Default::default(),
            oauth_buttons: Vec::new(),
            mcp_buttons: Vec::new(),
        };
        me.set_busy(true, ctx);
        WarpAgentPageView::start_provider_server(ctx);
        me
    }

    fn update(
        &mut self,
        snapshot: Snapshot,
        populate_form: bool,
        ctx: &mut ViewContext<WarpAgentPageView>,
    ) {
        self.snapshot = snapshot;
        for provider in &self.snapshot.providers {
            self.provider_rows.entry(provider.id.clone()).or_default();
            for model in provider.models.keys() {
                self.model_rows.entry(model.clone()).or_default();
            }
        }
        self.provider_rows
            .entry("custom-mywarp".into())
            .or_default();
        let mut items: Vec<_> = self
            .snapshot
            .providers
            .iter()
            .map(|p| {
                DropdownItem::new(
                    format!(
                        "{}{}",
                        p.name,
                        if p.has_api_key {
                            "  · Key available"
                        } else if p.connected {
                            "  · Available"
                        } else {
                            ""
                        }
                    ),
                    action(ProviderSetupAction::SelectProvider(p.id.clone())),
                )
            })
            .collect();
        items.push(DropdownItem::new(
            "Custom OpenAI-compatible endpoint",
            action(ProviderSetupAction::SelectProvider("custom-mywarp".into())),
        ));
        self.provider_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_items(items, ctx);
            dropdown.reset_selection(ctx);
        });
        self.mode_dropdown.update(ctx, |dropdown, ctx| {
            let items = self
                .snapshot
                .agents
                .keys()
                .map(|name| {
                    DropdownItem::new(
                        match name.as_str() {
                            "build" => "Build — edit files and run tools".into(),
                            "plan" => "Plan — research before making changes".into(),
                            _ => name.clone(),
                        },
                        action(ProviderSetupAction::AgentMode(name.clone())),
                    )
                })
                .collect();
            dropdown.set_items(items, ctx);
            dropdown.set_selected_by_action(
                action(ProviderSetupAction::AgentMode(self.snapshot.agent.clone())),
                ctx,
            );
        });
        let policy = warp_core::provider_setup::read_tool_policy().unwrap_or_else(|_| "ask".into());
        self.policy_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_selected_by_action(action(ProviderSetupAction::ToolPolicy(policy)), ctx)
        });
        self.update_selection(populate_form, ctx);
        self.mcp_buttons = self
            .snapshot
            .mcp
            .keys()
            .map(|_| Default::default())
            .collect();
        self.set_busy(self.busy, ctx);
    }

    fn update_selection(&mut self, populate_form: bool, ctx: &mut ViewContext<WarpAgentPageView>) {
        let provider = self
            .snapshot
            .providers
            .iter()
            .find(|p| p.id == self.selected);
        self.oauth_buttons = provider
            .map(|p| {
                p.oauth
                    .iter()
                    .map(|_| MouseStateHandle::default())
                    .collect()
            })
            .unwrap_or_default();
        let items: Vec<_> = std::iter::once(DropdownItem::new(
            "Choose a model…",
            action(ProviderSetupAction::SelectModel(String::new())),
        ))
        .chain(
            provider
                .into_iter()
                .flat_map(|p| p.models.iter())
                .map(|(id, label)| {
                    DropdownItem::new(
                        format!("{label}  ·  {id}"),
                        action(ProviderSetupAction::SelectModel(id.clone())),
                    )
                }),
        )
        .collect();
        let default_prefix = format!("{}/", self.selected);
        if !provider.is_some_and(|p| p.models.contains_key(&self.model)) {
            self.model = self
                .snapshot
                .default_model
                .strip_prefix(&default_prefix)
                .filter(|id| provider.is_some_and(|p| p.models.contains_key(*id)))
                .map(str::to_owned)
                .or_else(|| {
                    provider
                        .filter(|p| self.selected.starts_with("custom-") && p.models.len() == 1)
                        .and_then(|p| p.models.keys().next().cloned())
                })
                .unwrap_or_default();
        }
        self.model_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_items(items, ctx);
            dropdown.set_selected_by_action(
                action(ProviderSetupAction::SelectModel(self.model.clone())),
                ctx,
            );
        });
        if !self.model.is_empty() {
            self.model_scroll.scroll_to_position(ScrollTarget {
                position_id: format!("mywarp-model-{}", self.model),
                mode: ScrollToPositionMode::FullyIntoView,
            });
        }
        if populate_form {
            self.populate_connection_form(false, ctx);
        }
    }

    fn populate_connection_form(
        &mut self,
        preserve_edits: bool,
        ctx: &mut ViewContext<WarpAgentPageView>,
    ) {
        let provider = self
            .snapshot
            .providers
            .iter()
            .find(|provider| provider.id == self.selected);
        let endpoint = provider
            .map(|provider| provider.endpoint.clone())
            .unwrap_or_default();
        refresh_editor(
            &self.endpoint,
            &mut self.loaded_endpoint,
            endpoint,
            preserve_edits,
            ctx,
        );
        let model = if self.selected.starts_with("custom-") {
            self.model.clone()
        } else {
            String::new()
        };
        refresh_editor(
            &self.custom_model,
            &mut self.loaded_custom_model,
            model,
            preserve_edits,
            ctx,
        );
        self.key.update(ctx, |editor, ctx| {
            if !preserve_edits {
                editor.set_buffer_text("", ctx);
            }
            editor.set_placeholder_text(
                if provider.is_some_and(|provider| provider.has_api_key) {
                    "********"
                } else {
                    "Enter API key"
                },
                ctx,
            );
        });
    }

    fn set_busy(&mut self, busy: bool, ctx: &mut ViewContext<WarpAgentPageView>) {
        self.busy = busy;
        let blocked = busy || self.authorization.is_some();
        for dropdown in [&self.provider_dropdown, &self.model_dropdown] {
            dropdown.update(ctx, |d, ctx| {
                if blocked {
                    d.set_disabled(ctx)
                } else {
                    d.set_enabled(ctx)
                }
            });
        }
        for dropdown in [
            &self.mode_dropdown,
            &self.policy_dropdown,
            &self.mcp_type_dropdown,
        ] {
            dropdown.update(ctx, |d, ctx| {
                if blocked {
                    d.set_disabled(ctx)
                } else {
                    d.set_enabled(ctx)
                }
            });
        }
        for editor in [
            &self.key,
            &self.endpoint,
            &self.custom_model,
            &self.instructions,
            &self.mcp_name,
            &self.mcp_command,
            &self.mcp_arguments,
            &self.profile_name,
            &self.profile_prompt,
        ] {
            update_editor_interaction_state(editor.clone(), !blocked, ctx);
        }
        update_editor_interaction_state(self.code.clone(), !busy, ctx);
    }

    fn text(
        text: impl Into<String>,
        size: f32,
        subtle: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        Text::new(text.into(), appearance.ui_font_family(), size)
            .with_color(if subtle {
                appearance.theme().nonactive_ui_text_color().into_solid()
            } else {
                appearance.theme().active_ui_text_color().into_solid()
            })
            .finish()
    }

    fn field(
        label: &str,
        editor: &ViewHandle<EditorView>,
        height: f32,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        Flex::column()
            .with_child(Self::text(label, 13., false, appearance))
            .with_child(
                Container::new(
                    ConstrainedBox::new(
                        SavePosition::new(
                            ChildView::new(editor).finish(),
                            &format!("mywarp-provider-field-{label}"),
                        )
                        .finish(),
                    )
                    .with_min_height(height)
                    .finish(),
                )
                .with_uniform_padding(10.)
                .with_background(appearance.theme().surface_1())
                .with_border(Border::all(1.).with_border_fill(appearance.theme().surface_2()))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                .with_margin_top(8.)
                .with_margin_bottom(18.)
                .finish(),
            )
            .finish()
    }

    fn button(
        &self,
        label: &str,
        index: usize,
        command: ProviderSetupAction,
        primary: bool,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let disabled = self.busy
            || ((self.model.is_empty()
                || !self
                    .snapshot
                    .providers
                    .iter()
                    .any(|p| p.id == self.selected && p.connected))
                && matches!(
                    command,
                    ProviderSetupAction::Test | ProviderSetupAction::DefaultModel
                ))
            || (self.authorization.is_some()
                && !matches!(
                    command,
                    ProviderSetupAction::FinishAuthorization
                        | ProviderSetupAction::CancelAuthorization
                ));
        button(
            label,
            self.buttons[index].clone(),
            command,
            primary,
            disabled,
            appearance,
        )
    }

    fn feedback_for(&self, scope: FeedbackScope, appearance: &Appearance) -> Box<dyn Element> {
        if self.feedback_scope == scope {
            self.feedback(appearance)
        } else {
            Empty::new().finish()
        }
    }

    fn feedback(&self, appearance: &Appearance) -> Box<dyn Element> {
        let color = if self.error {
            appearance.theme().ui_error_color()
        } else {
            appearance.theme().nonactive_ui_text_color().into_solid()
        };
        SavePosition::new(
            Text::new(self.status.clone(), appearance.ui_font_family(), 13.)
                .with_color(color)
                .finish(),
            if self.connection_check.is_some() {
                "mywarp-connection-testing"
            } else if self.status == "Connection test cancelled." {
                "mywarp-connection-cancelled"
            } else if self.error && self.feedback_scope == FeedbackScope::Models {
                "mywarp-connection-error"
            } else if !self.error && self.status.starts_with("Connection verified") {
                "mywarp-connection-verified"
            } else {
                "mywarp-provider-feedback"
            },
        )
        .finish()
    }

    fn connection_form(&self, appearance: &Appearance) -> Box<dyn Element> {
        let provider = self
            .snapshot
            .providers
            .iter()
            .find(|p| p.id == self.selected);
        let mut column = Flex::column();
        let name = provider
            .map(|p| p.name.as_str())
            .unwrap_or("Custom endpoint");
        let connected = provider.is_some_and(|p| p.connected);
        column.add_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Expanded::new(1., Self::text(name, 17., false, appearance)).finish())
                .with_child(Self::text(
                    if self
                        .snapshot
                        .verified
                        .contains(&(self.selected.clone(), self.model.clone()))
                    {
                        "● Verified"
                    } else if provider.is_some_and(|provider| provider.has_api_key) {
                        "Key available · Not tested"
                    } else if connected {
                        "Available · Not tested"
                    } else {
                        "No credentials"
                    },
                    12.,
                    !connected,
                    appearance,
                ))
                .finish(),
        );
        column.add_child(
            Container::new(Self::text(
                provider
                    .map(|p| p.credential.as_str())
                    .unwrap_or("Connect any service that supports OpenAI Chat Completions."),
                12.,
                true,
                appearance,
            ))
            .with_margin_top(6.)
            .with_margin_bottom(20.)
            .finish(),
        );
        column.add_child(Self::field("Base URL", &self.endpoint, 18., appearance));
        column.add_child(
            Container::new(Self::text(
                match self.selected.as_str() {
                    "zai-coding-plan" | "zai" => "API format: OpenAI Chat Completions",
                    "anthropic" => "API format: Anthropic Messages",
                    _ => "API format: Provider default",
                },
                12.,
                true,
                appearance,
            ))
            .with_margin_bottom(16.)
            .finish(),
        );
        column.add_child(
            SavePosition::new(
                Self::field("API key", &self.key, 18., appearance),
                if provider.is_some_and(|provider| provider.has_api_key) {
                    "mywarp-provider-key-saved"
                } else {
                    "mywarp-provider-key-empty"
                },
            )
            .finish(),
        );
        if provider.is_some_and(|provider| provider.has_api_key) {
            column.add_child(
                Container::new(button(
                    "Open key file",
                    self.buttons[14].clone(),
                    ProviderSetupAction::OpenApiKeyFile,
                    false,
                    false,
                    appearance,
                ))
                .with_margin_bottom(14.)
                .finish(),
            );
        }
        if matches!(self.selected.as_str(), "zai" | "zai-coding-plan") {
            column.add_child(Container::new(Self::text(
                if self.selected == "zai-coding-plan" {
                    "Uses your GLM Coding Plan quota. Enter a key from Personal Coding Plan > Plan Overview."
                } else {
                    "Uses prepaid API balance. For a coding subscription, choose Z.AI Coding Plan in the provider list."
                }, 12., true, appearance,
            )).with_margin_bottom(16.).finish());
        }
        if self.selected.starts_with("custom-") {
            column.add_child(Self::field(
                "Custom model ID",
                &self.custom_model,
                18.,
                appearance,
            ));
        }
        let mut buttons = Flex::row().with_child(self.button(
            "Save connection",
            0,
            ProviderSetupAction::Save,
            true,
            appearance,
        ));
        if connected {
            buttons.add_child(
                Container::new(self.button(
                    "Disconnect",
                    2,
                    ProviderSetupAction::Disconnect,
                    false,
                    appearance,
                ))
                .with_margin_left(10.)
                .finish(),
            );
        }
        column.add_child(buttons.finish());
        if let Some(provider) = provider {
            for ((index, label), mouse) in provider.oauth.iter().zip(&self.oauth_buttons) {
                column.add_child(
                    Container::new(button(
                        label,
                        mouse.clone(),
                        ProviderSetupAction::Authorize(*index),
                        false,
                        self.busy,
                        appearance,
                    ))
                    .with_margin_top(12.)
                    .finish(),
                );
            }
        }
        if self.selected == "anthropic" {
            column.add_child(Container::new(Self::text("Claude API keys connect here. Claude Code's own subscription login is available under Third party CLI agents.",12.,true,appearance)).with_margin_top(14.).finish());
        }
        if let Some((_, _, instructions, needs_code)) = &self.authorization {
            column.add_child(
                Container::new(Self::text(instructions, 13., false, appearance))
                    .with_margin_top(18.)
                    .finish(),
            );
            if *needs_code {
                column.add_child(Self::field(
                    "Authorization code",
                    &self.code,
                    18.,
                    appearance,
                ));
            }
            column.add_child(
                Flex::row()
                    .with_child(self.button(
                        "Complete sign-in",
                        6,
                        ProviderSetupAction::FinishAuthorization,
                        true,
                        appearance,
                    ))
                    .with_child(
                        Container::new(self.button(
                            "Cancel",
                            7,
                            ProviderSetupAction::CancelAuthorization,
                            false,
                            appearance,
                        ))
                        .with_margin_left(10.)
                        .finish(),
                    )
                    .finish(),
            );
        }
        column.add_child(
            Container::new(self.feedback_for(FeedbackScope::Connection, appearance))
                .with_margin_top(18.)
                .finish(),
        );
        column.add_child(
            Container::new(self.models(appearance))
                .with_margin_top(24.)
                .finish(),
        );
        column.finish()
    }

    fn providers(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut rail = Flex::column().with_child(
            Container::new(Self::text("Providers", 12., true, appearance))
                .with_margin_bottom(14.)
                .finish(),
        );
        let recommended = [
            "zai-coding-plan",
            "zai",
            "openai",
            "anthropic",
            "openrouter",
        ];
        for id in recommended.into_iter().chain(
            self.snapshot
                .providers
                .iter()
                .filter(|p| {
                    (p.connected || p.id == self.selected) && !recommended.contains(&p.id.as_str())
                })
                .map(|p| p.id.as_str()),
        ) {
            let Some(mouse) = self.provider_rows.get(id) else {
                continue;
            };
            let provider = self.snapshot.providers.iter().find(|p| p.id == id);
            let label = match id {
                "zai-coding-plan" => "Z.AI Coding Plan",
                "zai" => "Z.AI Standard API",
                "openai" => "OpenAI / Codex",
                "anthropic" => "Anthropic / Claude",
                "openrouter" => "OpenRouter",
                _ => provider.map(|p| p.name.as_str()).unwrap_or(id),
            };
            let marker = provider_marker(&self.snapshot, id);
            rail.add_child(Self::selection_row(
                label,
                SavePosition::new(
                    match marker {
                        ProviderMarker::Default => Text::new("●", appearance.ui_font_family(), 12.)
                            .with_color(appearance.theme().ui_green_color())
                            .finish(),
                        ProviderMarker::Verified => {
                            Text::new("●", appearance.ui_font_family(), 12.)
                                .with_color(appearance.theme().ui_yellow_color())
                                .finish()
                        }
                        ProviderMarker::Untested => Self::text("○", 12., true, appearance),
                    },
                    &format!("mywarp-provider-marker-{id}-{marker:?}"),
                )
                .finish(),
                mouse.clone(),
                ProviderSetupAction::SelectProvider(id.into()),
                self.selected == id,
                &format!("mywarp-provider-action-{label}"),
                appearance,
            ));
        }
        if self.selected == "custom-mywarp"
            && !self
                .snapshot
                .providers
                .iter()
                .any(|p| p.id == self.selected)
            && let Some(mouse) = self.provider_rows.get("custom-mywarp")
        {
            rail.add_child(Self::selection_row(
                "Custom endpoint",
                Self::text("○", 12., true, appearance),
                mouse.clone(),
                ProviderSetupAction::SelectProvider("custom-mywarp".into()),
                true,
                "mywarp-provider-action-Custom endpoint",
                appearance,
            ));
        }
        let panel = Flex::row()
            .with_child(
                ConstrainedBox::new(
                    Container::new(rail.finish())
                        .with_uniform_padding(16.)
                        .finish(),
                )
                .with_width(210.)
                .finish(),
            )
            .with_child(
                Expanded::new(
                    1.,
                    Container::new(self.connection_form(appearance))
                        .with_uniform_padding(24.)
                        .with_border(
                            Border::left(1.).with_border_fill(appearance.theme().surface_2()),
                        )
                        .finish(),
                )
                .finish(),
            );
        Flex::column()
            .with_child(Self::text(
                "Manage your AI providers. Choose a connection to configure and test its models.",
                13.,
                true,
                appearance,
            ))
            .with_child(
                Container::new(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_child(
                            Container::new(button(
                                "Open provider config",
                                self.buttons[13].clone(),
                                ProviderSetupAction::OpenConfiguration,
                                false,
                                false,
                                appearance,
                            ))
                            .with_margin_right(10.)
                            .finish(),
                        )
                        .with_child(self.button(
                            "Refresh",
                            1,
                            ProviderSetupAction::Refresh,
                            false,
                            appearance,
                        ))
                        .with_child(
                            Container::new(
                                SavePosition::new(
                                    ChildView::new(&self.provider_dropdown).finish(),
                                    "mywarp-provider-picker",
                                )
                                .finish(),
                            )
                            .with_margin_left(10.)
                            .finish(),
                        )
                        .finish(),
                )
                .with_margin_top(12.)
                .finish(),
            )
            .with_child(
                Container::new(
                    SavePosition::new(
                        Container::new(panel.finish())
                            .with_border(
                                Border::all(1.).with_border_fill(appearance.theme().surface_2()),
                            )
                            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
                            .finish(),
                        if !self.busy && self.server.is_some() {
                            "mywarp-providers-ready"
                        } else {
                            "mywarp-providers-loading"
                        },
                    )
                    .finish(),
                )
                .with_margin_top(18.)
                .finish(),
            )
            .finish()
    }

    fn selection_row(
        label: &str,
        trailing: Box<dyn Element>,
        mouse: MouseStateHandle,
        command: ProviderSetupAction,
        selected: bool,
        position: &str,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        SavePosition::new(
            Hoverable::new(mouse, |state| {
                let body = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Expanded::new(1., Self::text(label, 13., false, appearance)).finish(),
                    )
                    .with_child(trailing);
                let mut row = Container::new(body.finish());
                if selected || state.is_hovered() {
                    row = row.with_background(appearance.theme().surface_2());
                }
                row.with_horizontal_padding(12.)
                    .with_vertical_padding(12.)
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
                    .with_margin_bottom(6.)
                    .finish()
            })
            .with_cursor(Cursor::PointingHand)
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action(command.clone())))
            .finish(),
            position,
        )
        .finish()
    }

    fn models(&self, appearance: &Appearance) -> Box<dyn Element> {
        let mut column =
            Flex::column().with_child(Self::text("Model list", 15., false, appearance));
        let mut rows = Flex::column();
        let provider = self
            .snapshot
            .providers
            .iter()
            .find(|p| p.id == self.selected);
        if let Some(provider) = provider {
            if provider.models.len() > 8 {
                column.add_child(
                    Container::new(ChildView::new(&self.model_dropdown).finish())
                        .with_margin_top(12.)
                        .with_margin_bottom(8.)
                        .finish(),
                );
            }
            for (id, label) in &provider.models {
                if provider.models.len() > 8 && *id != self.model {
                    continue;
                }
                if let Some(mouse) = self.model_rows.get(id) {
                    rows.add_child(Self::selection_row(
                        &format!("{label}  ·  {id}"),
                        Self::text(
                            if *id == self.model { "Selected" } else { "" },
                            12.,
                            true,
                            appearance,
                        ),
                        mouse.clone(),
                        ProviderSetupAction::SelectModel(id.clone()),
                        *id == self.model,
                        &format!("mywarp-model-{id}"),
                        appearance,
                    ));
                }
            }
            if provider.models.is_empty() {
                column.add_child(Self::text("No models are available. Save this connection or refresh the provider directory.", 12., true, appearance));
            }
        }
        column.add_child(
            Container::new(
                ConstrainedBox::new(
                    NewScrollable::vertical(
                        SingleAxisConfig::Clipped {
                            handle: self.model_scroll.clone(),
                            child: rows.finish(),
                        },
                        appearance.theme().nonactive_ui_detail().into(),
                        appearance.theme().active_ui_detail().into(),
                        warpui::elements::Fill::None,
                    )
                    .finish(),
                )
                .with_max_height(150.)
                .finish(),
            )
            .with_border(Border::all(1.).with_border_fill(appearance.theme().surface_2()))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .with_uniform_padding(4.)
            .with_margin_top(10.)
            .finish(),
        );
        column.add_child(
            Container::new(
                Flex::row()
                    .with_child(self.button(
                        if self.connection_check.is_some() {
                            "Testing…"
                        } else {
                            "Test connection"
                        },
                        3,
                        ProviderSetupAction::Test,
                        false,
                        appearance,
                    ))
                    .with_child(
                        Container::new(self.button(
                            "Set as default",
                            4,
                            ProviderSetupAction::DefaultModel,
                            true,
                            appearance,
                        ))
                        .with_margin_left(10.)
                        .finish(),
                    )
                    .finish(),
            )
            .with_margin_top(14.)
            .finish(),
        );
        if self.connection_check.is_some() {
            column.add_child(
                Container::new(button(
                    "Cancel test",
                    self.cancel_test.clone(),
                    ProviderSetupAction::CancelTest,
                    false,
                    false,
                    appearance,
                ))
                .with_margin_top(10.)
                .finish(),
            );
        }
        column.add_child(
            Container::new(self.feedback_for(FeedbackScope::Models, appearance))
                .with_margin_top(12.)
                .finish(),
        );
        column.add_child(
            Container::new(Self::text(
                format!(
                    "Default for new chats: {}",
                    if self.snapshot.default_model.is_empty() {
                        "Not selected"
                    } else {
                        &self.snapshot.default_model
                    }
                ),
                12.,
                true,
                appearance,
            ))
            .with_margin_top(12.)
            .finish(),
        );
        column.finish()
    }
}

fn refresh_editor(
    editor: &ViewHandle<EditorView>,
    loaded: &mut String,
    value: String,
    preserve_edits: bool,
    ctx: &mut ViewContext<WarpAgentPageView>,
) {
    if !preserve_edits || editor.as_ref(ctx).buffer_text(ctx) == *loaded {
        editor.update(ctx, |editor, ctx| editor.set_buffer_text(&value, ctx));
    }
    *loaded = value;
}

fn button(
    label: &str,
    mouse: MouseStateHandle,
    command: ProviderSetupAction,
    primary: bool,
    disabled: bool,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let command_id = format!("{command:?}");
    let mut b = appearance
        .ui_builder()
        .button(
            if primary {
                ButtonVariant::Accent
            } else {
                ButtonVariant::Secondary
            },
            mouse,
        )
        .with_text_label(label.into())
        .with_style(UiComponentStyles {
            height: Some(36.),
            font_size: Some(13.),
            ..Default::default()
        });
    if disabled {
        b = b.disabled();
    }
    let position_id = if matches!(command, ProviderSetupAction::SelectProvider(_)) {
        format!("mywarp-provider-action-{label}")
    } else {
        format!("mywarp-provider-action-{label}-{command_id}")
    };
    SavePosition::new(
        b.build()
            .on_click(move |ctx, _, _| ctx.dispatch_typed_action(action(command.clone())))
            .finish(),
        &position_id,
    )
    .finish()
}

impl WarpAgentPageView {
    #[cfg(feature = "provider-settings-check")]
    pub(in crate::settings_view) fn save_provider_with_configuration_reload(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        self.handle_provider_setup_action(&ProviderSetupAction::Save, ctx);
        crate::configuration_watcher::ConfigurationWatcher::handle(ctx).update(ctx, |_, ctx| {
            ctx.emit(crate::configuration_watcher::ConfigurationEvent::Changed);
        });
    }

    #[cfg(feature = "provider-settings-check")]
    pub(in crate::settings_view) fn provider_setup_ready(&self) -> bool {
        self.provider_setup
            .as_ref()
            .is_some_and(|state| state.server.is_some() && !state.busy && !state.error)
    }

    #[cfg(feature = "provider-settings-check")]
    pub(in crate::settings_view) fn provider_key_is_masked(&self, ctx: &AppContext) -> bool {
        self.provider_setup.as_ref().is_some_and(|state| {
            let editor = state.key.as_ref(ctx);
            state
                .snapshot
                .providers
                .iter()
                .any(|provider| provider.id == state.selected && provider.has_api_key)
                && editor.buffer_text(ctx).is_empty()
                && editor.placeholder_text("") == Some("********")
        })
    }

    #[cfg(feature = "provider-settings-check")]
    pub(crate) fn provider_configuration_matches(
        &self,
        model: &str,
        draft: &str,
        invalid: bool,
        ctx: &AppContext,
    ) -> bool {
        self.provider_setup.as_ref().is_some_and(|state| {
            !state.busy
                && !state.reload_pending
                && state.key.as_ref(ctx).buffer_text(ctx) == draft
                && if invalid {
                    state.error && state.status.starts_with("Configuration was not reloaded:")
                } else {
                    !state.error
                        && state.snapshot.providers.iter().any(|provider| {
                            provider.id == "custom-mywarp" && provider.models.contains_key(model)
                        })
                }
        })
    }

    fn refresh_provider_configuration(&mut self, ctx: &mut ViewContext<Self>) {
        let Some(state) = self.provider_setup.as_mut() else {
            return;
        };
        if !state.reload_pending || state.busy || state.authorization.is_some() {
            return;
        }
        state.reload_pending = false;
        let generation = state.configuration_generation;
        state.set_busy(true, ctx);
        state.feedback_scope = FeedbackScope::Connection;
        state.status = "Reloading changed provider configuration…".into();
        ctx.spawn(offload(|| {
            let profile = warp_core::provider_setup::profile_dir()?;
            let server = ProviderServer::start(profile.clone(),warp_core::provider_setup::runtime_executable())?;
            let snapshot = server.snapshot()?;
            let instructions = std::fs::read_to_string(warp_core::windows_package::configuration_dir(&profile).join("agent-instructions.md")).unwrap_or_default();
            Ok((server,snapshot,instructions))
        }), move |me,result,ctx| {
            let Some(state) = me.provider_setup.as_mut() else { return; };
            state.set_busy(false,ctx);
            if state.configuration_generation == generation {
                match result {
                    Ok((server,snapshot,instructions)) => {
                        state.server = Some(server);
                        state.update(snapshot,false,ctx);
                        state.populate_connection_form(true,ctx);
                        refresh_editor(&state.instructions,&mut state.loaded_instructions,instructions,true,ctx);
                        if state.snapshot.verified.contains(&(state.selected.clone(),state.model.clone())) {
                            state.status = "Connection verified — the selected model responded successfully.".into();
                            state.feedback_scope = FeedbackScope::Models;
                        } else { state.status = "Configuration reloaded from disk.".into(); }
                        state.error = false;
                    }
                    Err(error) => { state.status = format!("Configuration was not reloaded: {error}"); state.error = true; }
                }
            }
            ctx.notify();
            me.refresh_provider_configuration(ctx);
        });
        ctx.notify();
    }

    fn update_connection_check_progress(attempt: Arc<AtomicBool>, ctx: &mut ViewContext<Self>) {
        ctx.spawn(
            Timer::after(Duration::from_millis(250)),
            move |me, _, ctx| {
                let Some(state) = me.provider_setup.as_mut() else {
                    return;
                };
                let Some((started, cancelled)) = &state.connection_check else {
                    return;
                };
                if !Arc::ptr_eq(cancelled, &attempt) {
                    return;
                }
                if !cancelled.load(Ordering::Relaxed) {
                    state.status = format!(
                        "Waiting for {}… {}s / {}s. You can cancel this test.",
                        state.model,
                        started.elapsed().as_secs(),
                        CONNECTION_CHECK_TIMEOUT.as_secs(),
                    );
                }
                ctx.notify();
                Self::update_connection_check_progress(attempt, ctx);
            },
        );
    }

    fn start_provider_server(ctx: &mut ViewContext<Self>) {
        ctx.spawn(
            offload(|| {
                let server = ProviderServer::start(
                    warp_core::provider_setup::profile_dir()?,
                    warp_core::provider_setup::runtime_executable(),
                )?;
                let snapshot = server.snapshot()?;
                Ok((server, snapshot))
            }),
            |me, result, ctx| {
                if let Some(state) = me.provider_setup.as_mut() {
                    state.busy = false;
                    match result {
                        Ok((server, snapshot)) => {
                            state.server = Some(server);
                            if let Some((provider, _)) = snapshot.default_model.split_once('/') {
                                state.selected = provider.to_owned();
                            } else if let Some(provider) = [
                                "zai-coding-plan",
                                "openai",
                                "anthropic",
                                "openrouter",
                                "zai",
                            ]
                            .into_iter()
                            .find(|id| {
                                snapshot
                                    .providers
                                    .iter()
                                    .any(|p| p.id == *id && p.connected)
                            }) {
                                state.selected = provider.into();
                            }
                            state.status =
                                "Ready. Add a key or use an available provider sign-in.".into();
                            state.error = false;
                            state.update(snapshot, true, ctx);
                        }
                        Err(error) => {
                            state.status = error.to_string();
                            state.error = true;
                        }
                    }
                    ctx.notify();
                }
            },
        );
    }

    pub(super) fn handle_provider_setup_action(
        &mut self,
        value: &ProviderSetupAction,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(state) = self.provider_setup.as_mut() else {
            return;
        };
        if matches!(
            value,
            ProviderSetupAction::OpenConfiguration | ProviderSetupAction::OpenApiKeyFile
        ) {
            let id = state.selected.clone();
            let open_key = matches!(value, ProviderSetupAction::OpenApiKeyFile);
            ctx.spawn(
                offload(move || {
                    let profile = warp_core::provider_setup::profile_dir()?;
                    warp_core::windows_package::prepare_opencode_profile(&profile)?;
                    if open_key {
                        warp_core::provider_setup::api_key_configuration_file(&profile, &id)
                    } else {
                        Ok(warp_core::provider_setup::configuration_file(&profile))
                    }
                }),
                |me, result, ctx| match result {
                    Ok(path) => ctx.emit(WarpAgentPageEvent::OpenProviderConfigFile(path)),
                    Err(error) => {
                        if let Some(state) = me.provider_setup.as_mut() {
                            state.feedback_scope = FeedbackScope::Connection;
                            state.status = error.to_string();
                            state.error = true;
                        }
                        ctx.notify();
                    }
                },
            );
            return;
        }
        if matches!(value, ProviderSetupAction::CancelTest) {
            if let Some((_, cancelled)) = &state.connection_check {
                cancelled.store(true, Ordering::Relaxed);
                state.status = "Cancelling connection test…".into();
                ctx.notify();
            }
            return;
        }
        if state.busy {
            return;
        }
        if state.authorization.is_some()
            && !matches!(
                value,
                ProviderSetupAction::FinishAuthorization | ProviderSetupAction::CancelAuthorization
            )
        {
            state.status =
                "Complete or cancel this provider sign-in before changing other settings.".into();
            ctx.notify();
            return;
        }
        state.feedback_scope = feedback_scope(value);
        match value {
            ProviderSetupAction::EditMcp(name) => {
                state.status.clear();
                state.error = false;
                if let Some(config) = state.snapshot.mcp.get(name) {
                    state.mcp_type = config["type"].as_str().unwrap_or("remote").into();
                    state.mcp_type_dropdown.update(ctx, |d, ctx| {
                        d.set_selected_by_action(
                            action(ProviderSetupAction::McpType(state.mcp_type.clone())),
                            ctx,
                        )
                    });
                    state
                        .mcp_name
                        .update(ctx, |e, ctx| e.set_buffer_text(name, ctx));
                    let command = config["command"].as_array();
                    let executable = if state.mcp_type == "remote" {
                        config["url"].as_str().unwrap_or_default()
                    } else {
                        command
                            .and_then(|items| items.first())
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    };
                    let arguments = command
                        .into_iter()
                        .flatten()
                        .skip(1)
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n");
                    state
                        .mcp_command
                        .update(ctx, |e, ctx| e.set_buffer_text(executable, ctx));
                    state
                        .mcp_arguments
                        .update(ctx, |e, ctx| e.set_buffer_text(&arguments, ctx));
                }
                ctx.notify();
                return;
            }
            ProviderSetupAction::EditProfile => {
                state.status.clear();
                state.error = false;
                if let Some(profile) = state.snapshot.custom_agents.get(&state.snapshot.agent) {
                    state
                        .profile_name
                        .update(ctx, |e, ctx| e.set_buffer_text(&state.snapshot.agent, ctx));
                    state.profile_prompt.update(ctx, |e, ctx| {
                        e.set_buffer_text(profile["prompt"].as_str().unwrap_or_default(), ctx)
                    });
                    state.show_profile_editor = true;
                }
                ctx.notify();
                return;
            }
            ProviderSetupAction::McpType(kind) => {
                state.status.clear();
                state.error = false;
                state.mcp_type = kind.clone();
                state.mcp_type_dropdown.update(ctx, |d, ctx| {
                    d.set_selected_by_action(action(value.clone()), ctx)
                });
                state.mcp_command.update(ctx, |e, ctx| {
                    e.set_placeholder_text(
                        if kind == "remote" {
                            "https://your-server/mcp"
                        } else {
                            "Executable name or full path, e.g. npx"
                        },
                        ctx,
                    )
                });
                ctx.notify();
                return;
            }
            ProviderSetupAction::ShowProfileEditor(show) => {
                state.status.clear();
                state.error = false;
                state.show_profile_editor = *show;
                ctx.notify();
                return;
            }
            ProviderSetupAction::SelectProvider(id) => {
                state.selected = id.clone();
                state.model_scroll.scroll_to(Pixels::zero());
                state.authorization = None;
                state.model.clear();
                state.status = "Choose credentials and a model for this provider.".into();
                state.error = false;
                state.update(state.snapshot.clone(), true, ctx);
                ctx.notify();
                return;
            }
            ProviderSetupAction::SelectModel(id) => {
                state.status = if id.is_empty() {
                    "Choose a model before testing the connection."
                } else {
                    "Model selected. Test the connection or choose Set as default for new chats."
                }
                .into();
                state.error = false;
                state.model = id.clone();
                state.model_dropdown.update(ctx, |d, ctx| {
                    d.set_selected_by_action(action(value.clone()), ctx)
                });
                ctx.notify();
                return;
            }
            ProviderSetupAction::CancelAuthorization => {
                state.authorization = None;
                state.code.update(ctx, |e, ctx| e.set_buffer_text("", ctx));
            }
            _ => {}
        }
        let Some(server) = state.server.clone() else {
            state.set_busy(true, ctx);
            state.status = "Starting the AI runtime…".into();
            state.error = false;
            Self::start_provider_server(ctx);
            ctx.notify();
            return;
        };
        let populate_form = matches!(
            value,
            ProviderSetupAction::Save
                | ProviderSetupAction::Disconnect
                | ProviderSetupAction::Refresh
                | ProviderSetupAction::Authorize(_)
                | ProviderSetupAction::FinishAuthorization
                | ProviderSetupAction::CancelAuthorization
        );
        let saved_connection = matches!(value, ProviderSetupAction::Save);
        let command = value.clone();
        let id = state.selected.clone();
        let model = state.model.clone();
        let key = state.key.as_ref(ctx).buffer_text(ctx);
        if matches!(value, ProviderSetupAction::Save)
            && key.trim().is_empty()
            && !id.starts_with("custom-")
            && !state
                .snapshot
                .providers
                .iter()
                .any(|provider| provider.id == id && provider.connected)
        {
            state.status =
                "Enter an API key or use an available sign-in before saving this connection."
                    .into();
            state.error = true;
            ctx.notify();
            return;
        }
        let endpoint = state.endpoint.as_ref(ctx).buffer_text(ctx);
        let custom_model = state.custom_model.as_ref(ctx).buffer_text(ctx);
        if matches!(value, ProviderSetupAction::Test)
            && (!key.trim().is_empty()
                || state
                    .snapshot
                    .providers
                    .iter()
                    .find(|p| p.id == id)
                    .is_some_and(|p| p.endpoint != endpoint.trim()))
        {
            state.status = "Save connection before testing the changed key or URL.".into();
            state.error = true;
            ctx.notify();
            return;
        }
        let code = state.code.as_ref(ctx).buffer_text(ctx);
        let instructions = state.instructions.as_ref(ctx).buffer_text(ctx);
        let mcp_name = state.mcp_name.as_ref(ctx).buffer_text(ctx);
        let mcp_command = state.mcp_command.as_ref(ctx).buffer_text(ctx);
        let mcp_type = state.mcp_type.clone();
        let mcp_arguments = state.mcp_arguments.as_ref(ctx).buffer_text(ctx);
        let profile_name = state.profile_name.as_ref(ctx).buffer_text(ctx);
        let profile_prompt = state.profile_prompt.as_ref(ctx).buffer_text(ctx);
        let snapshot = state.snapshot.clone();
        let authorization = state.authorization.clone();
        let configuration_generation = state.configuration_generation;
        let test_cancelled = Arc::new(AtomicBool::new(false));
        if matches!(command, ProviderSetupAction::Test) {
            state.snapshot.verified.remove(&(id.clone(), model.clone()));
            state.connection_check = Some((Instant::now(), test_cancelled.clone()));
        }
        state.set_busy(true, ctx);
        state.error = false;
        state.status = if matches!(command, ProviderSetupAction::Test) {
            "Connecting to the selected model… 0s / 30s. You can cancel this test."
        } else {
            "Updating your profile…"
        }
        .into();
        if state.connection_check.is_some() {
            Self::update_connection_check_progress(test_cancelled.clone(), ctx);
        }
        ctx.spawn(offload(move || {
            let mut oauth = None;
            let mut message = "Saved. Configuration changes reload automatically; active responses finish first.".to_owned();
            let snapshot = match command {
                ProviderSetupAction::Refresh=>{server.restart()?;message="Provider directory refreshed.".into();server.snapshot()?},
                ProviderSetupAction::CancelAuthorization=>{server.restart()?;message="Provider sign-in cancelled.".into();server.snapshot()?},
                ProviderSetupAction::Save=>server.save_provider(&id,&key,&endpoint,&custom_model)?,
                ProviderSetupAction::Disconnect=>{message="Provider disconnected from this profile.".into();server.disconnect(&id)?},
                ProviderSetupAction::Test=>{
                    let profile = warp_core::provider_setup::profile_dir()?;
                    let current = ProviderServer::start(profile.clone(),warp_core::provider_setup::runtime_executable())?;
                    current.test_connection_cancellable(&id,&model,&test_cancelled)?;
                    message="Connection verified — the selected model responded successfully.".into();current.snapshot()?
                },
                ProviderSetupAction::DefaultModel=>{if model.is_empty(){bail!("Choose a model first.");}let snapshot=server.patch(json!({"model":format!("{id}/{model}")}))?;message=format!("Default model set to {id}/{model}. New chats will use this model.");snapshot},
                ProviderSetupAction::Authorize(method)=>{let result=server.authorize(&id,method)?;oauth=Some((id,method,result));message="Complete the provider's sign-in in your browser, then return here.".into();server.snapshot()?},
                ProviderSetupAction::FinishAuthorization=>{let (provider,method,_,_)=authorization.context("Start a provider sign-in first")?;server.complete_authorization(&provider,method,&code)?},
                ProviderSetupAction::AgentMode(mode)=>{warp_core::provider_setup::write_agent_override("default_agent",Value::String(mode.clone()))?;server.patch(json!({"default_agent":mode}))?},
                ProviderSetupAction::ToolPolicy(policy)=>{let permission=warp_core::provider_setup::write_tool_policy(&policy)?;server.patch(json!({"permission":permission}))?},
                ProviderSetupAction::SaveInstructions=>server.save_instructions(&instructions)?,
                ProviderSetupAction::AddMcp=>{warp_core::provider_setup::validate_provider_id(mcp_name.trim())?;let value=if mcp_type=="remote" {warp_core::provider_setup::validate_endpoint(&mcp_command)?;json!({"type":"remote","url":mcp_command.trim(),"enabled":true})}else{let command:Vec<String>=std::iter::once(mcp_command.trim().to_owned()).chain(mcp_arguments.lines().filter(|arg| !arg.trim().is_empty()).map(str::to_owned)).collect();if mcp_command.trim().is_empty(){bail!("The MCP command cannot be empty.");}json!({"type":"local","command":command,"enabled":true})};server.patch(json!({"mcp":{mcp_name.trim():value}}))?},
                ProviderSetupAction::ToggleMcp(name)=>{let enabled=snapshot.mcp.get(&name).is_some_and(|c|c["enabled"]==false);server.patch(json!({"mcp":{name:{"enabled":enabled}}}))?},
                ProviderSetupAction::RemoveProfile=>{
                    let name=snapshot.agent;
                    if matches!(name.as_str(),"build"|"plan") {bail!("Built-in profiles cannot be removed.");}
                    server.patch(json!({"default_agent":"build"}))?;
                    let result=server.remove_entry("agent",&name)?;
                    warp_core::provider_setup::write_agent_override("default_agent","build".into())?;
                    result
                },
                ProviderSetupAction::SaveProfile=>{
                    message="Profile saved. Select it as the default agent above.".into();
                    warp_core::provider_setup::validate_provider_id(profile_name.trim())?;
                    if matches!(profile_name.trim(),"build"|"plan") {bail!("Choose a new profile name; Build and Plan are built in.");}
                    if profile_prompt.trim().is_empty(){bail!("Describe the agent's role before saving this profile.");}
                    server.patch(json!({"agent":{profile_name.trim():{"description":format!("MyWarp {} profile",profile_name.trim()),"mode":"primary","prompt":profile_prompt.trim()}}}))?
                },
                ProviderSetupAction::RemoveMcp(name)=>server.remove_entry("mcp",&name)?,
                _=>server.snapshot()?,
            };
            Ok((snapshot,message,oauth))
        }),move |me,result,ctx| {
            let Some(state)=me.provider_setup.as_mut() else{return;}; state.busy=false;
            let cancelled=state.connection_check.take().is_some_and(|(_,cancelled)|cancelled.load(Ordering::Relaxed));
            if state.configuration_generation != configuration_generation && result.is_err() {
                state.set_busy(false,ctx);
                me.refresh_provider_configuration(ctx);
                ctx.notify();
                return;
            }
            match result {
                Ok((snapshot,message,oauth))=>{
                    if saved_connection {
                        // A newer snapshot does not undo the successful save of the disabled form.
                        state.key.update(ctx, |editor, ctx| editor.set_buffer_text("", ctx));
                        state.loaded_endpoint = state.endpoint.as_ref(ctx).buffer_text(ctx);
                        state.loaded_custom_model = state.custom_model.as_ref(ctx).buffer_text(ctx);
                    }
                    if state.configuration_generation != configuration_generation {
                        state.set_busy(false,ctx);
                        me.refresh_provider_configuration(ctx);
                        ctx.notify();
                        return;
                    }
                    state.status=message;state.error=false;
                    if let Some((id,method,result))=oauth {
                        let url=result["url"].as_str().unwrap_or_default();
                        if let Ok(url)=url::Url::parse(url) && url.scheme()=="https" && !warp_core::mywarp::is_warp_service(&url) {ctx.open_url(url.as_str());}
                        state.authorization=Some((id,method,result["instructions"].as_str().unwrap_or("Complete sign-in in your browser.").into(),result["method"]=="code"));
                    } else {state.authorization=None;}
                    state.update(snapshot,populate_form,ctx);
                },Err(error)=>{state.error = !cancelled;state.status=error.to_string();}
            }
            state.set_busy(false,ctx);
            ctx.notify();
            me.refresh_provider_configuration(ctx);
        });
        ctx.notify();
    }
}

pub(super) struct ConnectionWidget;
impl SettingsWidget for ConnectionWidget {
    type View = WarpAgentPageView;
    fn search_terms(&self) -> &str {
        "AI providers connect connection configuration config file edit API key OpenAI Codex Anthropic Claude OpenRouter Z.ai ZAI Coding Plan endpoint login sign-in model default catalog test verification"
    }
    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        view.provider_setup
            .as_ref()
            .map(|state| state.providers(appearance))
            .unwrap_or_else(|| Empty::new().finish())
    }
}

pub(super) struct AgentModeWidget;
impl SettingsWidget for AgentModeWidget {
    type View = WarpAgentPageView;
    fn search_terms(&self) -> &str {
        "agent behavior mode build plan default profile"
    }
    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let Some(state) = view.provider_setup.as_ref() else {
            return Empty::new().finish();
        };
        let mut column=Flex::column().with_child(ProviderSetup::text("Default agent profile",14.,false,appearance))
            .with_child(Container::new(ProviderSetup::text("Choose Build, Plan, or one of your saved profiles. Tool permissions also apply to custom profiles.",13.,true,appearance)).with_margin_top(6.).with_margin_bottom(12.).finish())
            .with_child(SavePosition::new(ChildView::new(&state.mode_dropdown).finish(),"mywarp-agent-profile-picker").finish());
        if state.show_profile_editor {
            column.add_child(
                Container::new(ProviderSetup::field(
                    "Profile name",
                    &state.profile_name,
                    18.,
                    appearance,
                ))
                .with_margin_top(18.)
                .finish(),
            );
            column.add_child(ProviderSetup::field(
                "Role and instructions",
                &state.profile_prompt,
                100.,
                appearance,
            ));
            column.add_child(
                Flex::row()
                    .with_child(state.button(
                        "Save profile",
                        9,
                        ProviderSetupAction::SaveProfile,
                        true,
                        appearance,
                    ))
                    .with_child(
                        Container::new(state.button(
                            "Close profile editor",
                            10,
                            ProviderSetupAction::ShowProfileEditor(false),
                            false,
                            appearance,
                        ))
                        .with_margin_left(10.)
                        .finish(),
                    )
                    .finish(),
            );
        } else {
            let mut buttons = Flex::row().with_spacing(10.).with_child(state.button(
                "New agent profile",
                9,
                ProviderSetupAction::ShowProfileEditor(true),
                false,
                appearance,
            ));
            if state
                .snapshot
                .custom_agents
                .contains_key(&state.snapshot.agent)
                && !matches!(state.snapshot.agent.as_str(), "build" | "plan")
            {
                buttons.add_child(state.button(
                    "Edit profile",
                    11,
                    ProviderSetupAction::EditProfile,
                    false,
                    appearance,
                ));
                buttons.add_child(state.button(
                    "Remove profile",
                    12,
                    ProviderSetupAction::RemoveProfile,
                    false,
                    appearance,
                ));
            }
            column.add_child(
                Container::new(buttons.finish())
                    .with_margin_top(14.)
                    .finish(),
            );
        }
        column.add_child(
            Container::new(state.feedback_for(FeedbackScope::Agent, appearance))
                .with_margin_top(12.)
                .finish(),
        );
        column.finish()
    }
}

pub(super) struct ToolPolicyWidget;
impl SettingsWidget for ToolPolicyWidget {
    type View = WarpAgentPageView;
    fn search_terms(&self) -> &str {
        "agent tools permissions approvals read only safety commands edits"
    }
    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let Some(state) = view.provider_setup.as_ref() else {
            return Empty::new().finish();
        };
        Flex::column().with_child(ProviderSetup::text("Tool permissions",14.,false,appearance))
            .with_child(Container::new(ProviderSetup::text("These rules apply to the native agent. Requests needing approval appear in the chat footer.",13.,true,appearance)).with_margin_top(6.).with_margin_bottom(12.).finish())
            .with_child(ChildView::new(&state.policy_dropdown).finish()).with_child(Container::new(state.feedback_for(FeedbackScope::Policy,appearance)).with_margin_top(12.).finish()).finish()
    }
}

pub(super) struct InstructionsWidget;
impl SettingsWidget for InstructionsWidget {
    type View = WarpAgentPageView;
    fn search_terms(&self) -> &str {
        "agent personal instructions rules knowledge context system prompt"
    }
    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let Some(state) = view.provider_setup.as_ref() else {
            return Empty::new().finish();
        };
        Flex::column()
            .with_child(ProviderSetup::field(
                "Personal agent instructions",
                &state.instructions,
                100.,
                appearance,
            ))
            .with_child(state.button(
                "Save instructions",
                5,
                ProviderSetupAction::SaveInstructions,
                true,
                appearance,
            ))
            .with_child(
                Container::new(state.feedback_for(FeedbackScope::Instructions, appearance))
                    .with_margin_top(12.)
                    .finish(),
            )
            .finish()
    }
}

pub(super) struct McpWidget;
impl SettingsWidget for McpWidget {
    type View = WarpAgentPageView;
    fn search_terms(&self) -> &str {
        "agent MCP servers tools local remote integrations"
    }
    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let Some(state) = view.provider_setup.as_ref() else {
            return Empty::new().finish();
        };
        let mut column=Flex::column().with_child(Container::new(ProviderSetup::text("Connect local commands or remote MCP endpoints. Enabled servers are loaded when the agent connects.",13.,true,appearance)).with_margin_bottom(16.).finish());
        for ((name, config), mouse) in state.snapshot.mcp.iter().zip(&state.mcp_buttons) {
            column.add_child(
                Container::new(
                    Flex::row()
                        .with_child(
                            Expanded::new(
                                1.,
                                ProviderSetup::text(
                                    format!(
                                        "{name}  ·  {}  ·  {}",
                                        config["type"].as_str().unwrap_or("server"),
                                        if config["enabled"] == false {
                                            "Disabled"
                                        } else {
                                            "Enabled"
                                        }
                                    ),
                                    14.,
                                    false,
                                    appearance,
                                ),
                            )
                            .finish(),
                        )
                        .with_child(
                            Flex::row()
                                .with_spacing(8.)
                                .with_child(button(
                                    "Edit",
                                    mouse[0].clone(),
                                    ProviderSetupAction::EditMcp(name.clone()),
                                    false,
                                    state.busy,
                                    appearance,
                                ))
                                .with_child(button(
                                    if config["enabled"] == false {
                                        "Enable"
                                    } else {
                                        "Disable"
                                    },
                                    mouse[1].clone(),
                                    ProviderSetupAction::ToggleMcp(name.clone()),
                                    false,
                                    state.busy,
                                    appearance,
                                ))
                                .with_child(button(
                                    "Remove",
                                    mouse[2].clone(),
                                    ProviderSetupAction::RemoveMcp(name.clone()),
                                    false,
                                    state.busy,
                                    appearance,
                                ))
                                .finish(),
                        )
                        .finish(),
                )
                .with_margin_bottom(12.)
                .finish(),
            );
        }
        column.add_child(ProviderSetup::field(
            "Server name",
            &state.mcp_name,
            18.,
            appearance,
        ));
        column.add_child(
            Container::new(
                SavePosition::new(
                    ChildView::new(&state.mcp_type_dropdown).finish(),
                    "mywarp-mcp-type-picker",
                )
                .finish(),
            )
            .with_margin_bottom(16.)
            .finish(),
        );
        column.add_child(ProviderSetup::field(
            if state.mcp_type == "remote" {
                "Server URL"
            } else {
                "Command"
            },
            &state.mcp_command,
            18.,
            appearance,
        ));
        if state.mcp_type == "local" {
            column.add_child(ProviderSetup::field(
                "Arguments — one per line",
                &state.mcp_arguments,
                80.,
                appearance,
            ));
        }
        column.add_child(state.button(
            "Add MCP server",
            8,
            ProviderSetupAction::AddMcp,
            true,
            appearance,
        ));
        column.add_child(
            Container::new(state.feedback_for(FeedbackScope::Mcp, appearance))
                .with_margin_top(12.)
                .finish(),
        );
        column.finish()
    }
}
