use settings::Setting as _;
use uuid::Uuid;
use warp_errors::report_if_error;
use warpui::elements::{
    ChildAnchor, ChildView, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
    CrossAxisAlignment, Empty, Flex, MainAxisSize, MouseStateHandle, ParentElement,
    PositionedElementAnchor, SavePosition, ScrollbarWidth, Wrap,
};
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::UiComponent as _;
use warpui::{
    AppContext, Element, Entity, EntityId, ModelHandle, SingletonEntity, TypedActionView, View,
    ViewContext, ViewHandle,
};

use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent_sdk::driver::harness::acp::desktop::DesktopAcp;
use crate::ai::blocklist::{BlocklistAIController, BlocklistAIHistoryModel};
use crate::ai::execution_profiles::ExecutionProfileId;
use crate::ai::execution_profiles::profiles::{
    AIExecutionProfilesModel, AIExecutionProfilesModelEvent,
};
use crate::ai::llms::{LLMId, LLMPreferences};
use crate::appearance::Appearance;
use crate::settings::{AISettings, NativeChatRuntime};
use crate::view_components::{Dropdown, DropdownItem};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ProviderAction {
    Runtime(NativeChatRuntime),
    Model(String),
    ReasoningEffort(String),
    Profile(ExecutionProfileId),
    ApplyProfile(ExecutionProfileId),
    AutoApprove(bool),
    ApplyAutoApprove(bool),
    ApplyRuntime(NativeChatRuntime),
    ApplyModel(String),
    ApplyReasoningEffort(String),
    Answer {
        conversation: AIConversationId,
        approval: Uuid,
        allow: bool,
    },
    Stop(AIConversationId),
}

pub(super) struct ProviderControls {
    terminal_id: EntityId,
    controller: ModelHandle<BlocklistAIController>,
    runtime: ModelHandle<DesktopAcp>,
    runtime_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    model_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    reasoning_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    profile_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    auto_approve_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    pending_profile_model: Option<String>,
    observed_profile_model: (ExecutionProfileId, Option<LLMId>),
    approve_mouse: MouseStateHandle,
    deny_mouse: MouseStateHandle,
    stop_mouse: MouseStateHandle,
    approval_scroll: ClippedScrollStateHandle,
    error: Option<String>,
}

impl ProviderControls {
    pub(super) fn new(
        terminal_id: EntityId,
        controller: ModelHandle<BlocklistAIController>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let runtime = controller.as_ref(ctx).provider_chat().clone();
        let runtime_dropdown = ctx.add_typed_action_view(Dropdown::new);
        let model_dropdown = ctx.add_typed_action_view(Dropdown::new);
        let reasoning_dropdown = ctx.add_typed_action_view(Dropdown::new);
        let profile_dropdown = ctx.add_typed_action_view(Dropdown::new);
        let auto_approve_dropdown = ctx.add_typed_action_view(Dropdown::new);
        let active = AIExecutionProfilesModel::as_ref(ctx).active_profile(Some(terminal_id), ctx);
        let observed_profile_model = (active.id().clone(), active.data().base_model.clone());
        ctx.subscribe_to_model(&AIExecutionProfilesModel::handle(ctx), |me, profiles, event, ctx| {
            let active = profiles.as_ref(ctx).active_profile(Some(me.terminal_id), ctx);
            let changed = matches!(event, AIExecutionProfilesModelEvent::ProfileUpdated(id) if id == active.id())
                || matches!(event, AIExecutionProfilesModelEvent::UpdatedActiveProfile { terminal_view_id } if *terminal_view_id == me.terminal_id)
                || matches!(event, AIExecutionProfilesModelEvent::ProfileDeleted);
            let current = (active.id().clone(), active.data().base_model.clone());
            if changed && current != me.observed_profile_model {
                me.observed_profile_model = current;
                me.queue_profile_model(ctx);
            }
            me.refresh(ctx);
        });
        ctx.subscribe_to_model(&runtime, |me, _, _, ctx| me.refresh(ctx));
        ctx.subscribe_to_model(&BlocklistAIHistoryModel::handle(ctx), |me, _, _, ctx| {
            me.refresh(ctx)
        });
        let mut me = Self {
            terminal_id,
            controller,
            runtime,
            runtime_dropdown,
            model_dropdown,
            reasoning_dropdown,
            profile_dropdown,
            auto_approve_dropdown,
            pending_profile_model: None,
            observed_profile_model,
            approve_mouse: Default::default(),
            deny_mouse: Default::default(),
            stop_mouse: Default::default(),
            approval_scroll: Default::default(),
            error: None,
        };
        me.refresh(ctx);
        me
    }

    fn conversation(&self, ctx: &AppContext) -> Option<AIConversationId> {
        BlocklistAIHistoryModel::as_ref(ctx).active_conversation_id(self.terminal_id)
    }

    pub(super) fn is_provider_chat(&self, ctx: &AppContext) -> bool {
        BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(self.terminal_id)
            .is_some_and(|conversation| conversation.provider_session().is_some())
    }

    pub(super) fn auto_approve_enabled(&self, ctx: &AppContext) -> bool {
        self.is_provider_chat(ctx)
            && self
                .controller
                .as_ref(ctx)
                .provider_auto_approve_enabled(ctx)
    }

    fn queue_profile_model(&mut self, ctx: &AppContext) {
        let profile =
            AIExecutionProfilesModel::as_ref(ctx).active_profile(Some(self.terminal_id), ctx);
        let model = profile.data().base_model.clone().unwrap_or_else(|| {
            LLMPreferences::as_ref(ctx)
                .get_default_base_model_for_team_uid(None, ctx)
                .id
                .clone()
        });
        self.pending_profile_model = Some(model.to_string());
    }

    fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
        let auto_approve = self
            .controller
            .as_ref(ctx)
            .provider_auto_approve_enabled(ctx);
        let editable = self
            .controller
            .as_ref(ctx)
            .provider_auto_approve_editable(ctx);
        self.auto_approve_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_menu_position(
                PositionedElementAnchor::TopLeft,
                ChildAnchor::BottomLeft,
                ctx,
            );
            dropdown.set_menu_header_text_override(|label| format!("Auto-approve: {label}"));
            dropdown.set_items(
                vec![
                    DropdownItem::new("Off", ProviderAction::AutoApprove(false)),
                    DropdownItem::new("On", ProviderAction::AutoApprove(true)),
                ],
                ctx,
            );
            dropdown.set_selected_by_action(ProviderAction::AutoApprove(auto_approve), ctx);
            if editable {
                dropdown.set_enabled(ctx);
            } else {
                dropdown.set_disabled(ctx);
            }
        });
        let profiles = AIExecutionProfilesModel::as_ref(ctx);
        let active = profiles
            .active_profile(Some(self.terminal_id), ctx)
            .id()
            .clone();
        let profile_items = profiles
            .get_all_profile_ids()
            .into_iter()
            .filter_map(|id| {
                profiles.get_profile_by_id(&id, ctx).map(|profile| {
                    let name = if profile.data().is_default_profile {
                        "Default".to_owned()
                    } else if profile.data().name.trim().is_empty() {
                        "Untitled".to_owned()
                    } else {
                        profile.data().name.clone()
                    };
                    DropdownItem::new(name, ProviderAction::Profile(id))
                })
            })
            .collect();
        let busy = self
            .conversation(ctx)
            .and_then(|id| self.runtime.as_ref(ctx).session(id))
            .is_some_and(|session| session.busy || !session.approvals.is_empty());
        self.profile_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_top_bar_max_width(180.);
            dropdown.set_menu_position(
                PositionedElementAnchor::TopLeft,
                ChildAnchor::BottomLeft,
                ctx,
            );
            dropdown.set_menu_header_text_override(|label| format!("Profile: {label}"));
            dropdown.set_items(profile_items, ctx);
            dropdown.set_selected_by_action(ProviderAction::Profile(active), ctx);
            if busy {
                dropdown.set_disabled(ctx);
            } else {
                dropdown.set_enabled(ctx);
            }
        });
        if let Some(model) = self.pending_profile_model.clone()
            && let Some(id) = self.conversation(ctx)
            && let Some(session) = self.runtime.as_ref(ctx).session(id)
            && !session.busy
            && !session.models.is_empty()
        {
            self.pending_profile_model = None;
            if session.model_id.as_ref() != Some(&model) {
                self.runtime
                    .update(ctx, |runtime, ctx| runtime.select_model(id, model, ctx));
            }
        }
        let state = BlocklistAIHistoryModel::as_ref(ctx)
            .active_conversation(self.terminal_id)
            .map(|conversation| {
                (
                    conversation.id(),
                    conversation.exchange_count() > 0,
                    conversation.provider_session().is_some(),
                )
            });
        if let Some((id, _, true)) = state
            && self.runtime.as_ref(ctx).session(id).is_none()
        {
            self.controller.update(ctx, |controller, ctx| {
                controller.ensure_provider_chat(id, ctx)
            });
        }
        let selected =
            if warp_core::mywarp::enabled() || state.is_some_and(|(_, _, provider)| provider) {
                NativeChatRuntime::OpenCode
            } else {
                NativeChatRuntime::Warp
            };
        self.runtime_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_top_bar_max_width(130.);
            dropdown.set_menu_position(
                PositionedElementAnchor::TopLeft,
                ChildAnchor::BottomLeft,
                ctx,
            );
            let mut runtimes = vec![DropdownItem::new(
                "MyWarp AI",
                ProviderAction::Runtime(NativeChatRuntime::OpenCode),
            )];
            if !warp_core::mywarp::enabled() {
                runtimes.insert(
                    0,
                    DropdownItem::new(
                        "Warp Agent",
                        ProviderAction::Runtime(NativeChatRuntime::Warp),
                    ),
                );
            }
            dropdown.set_items(runtimes, ctx);
            dropdown.set_selected_by_action(ProviderAction::Runtime(selected), ctx);
            if state.is_none_or(|(_, started, _)| started) {
                dropdown.set_disabled(ctx);
            } else {
                dropdown.set_enabled(ctx);
            }
        });
        let session = state.and_then(|(id, _, _)| self.runtime.as_ref(ctx).session(id));
        let choices = session
            .map(|session| {
                session
                    .models
                    .iter()
                    .filter(|choice| Some(&choice.id) == session.model_id.as_ref())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let selected = session.and_then(|session| session.model_id.clone());
        let reasoning_efforts = session
            .map(|session| session.reasoning_efforts.clone())
            .unwrap_or_default();
        let reasoning_effort = session.and_then(|session| session.reasoning_effort.clone());
        let connecting = session.is_none_or(|session| !session.ready && session.error.is_none());
        let busy = session.is_none_or(|session| session.busy)
            || state.is_none_or(|(id, _, _)| !self.runtime.as_ref(ctx).is_connected(id));
        self.model_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_top_bar_max_width(320.);
            dropdown.set_menu_width(460., ctx);
            dropdown.set_menu_position(
                PositionedElementAnchor::TopLeft,
                ChildAnchor::BottomLeft,
                ctx,
            );
            dropdown.set_menu_max_height(300., ctx);
            dropdown.set_items(
                choices
                    .iter()
                    .map(|choice| {
                        DropdownItem::new(
                            choice.label.clone(),
                            ProviderAction::Model(choice.id.clone()),
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_to_none(ctx);
            if let Some(selected) = selected {
                dropdown.set_selected_by_action(ProviderAction::Model(selected), ctx);
            }
            dropdown.set_disabled(ctx);
        });
        self.reasoning_dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_top_bar_max_width(170.);
            dropdown.set_menu_position(
                PositionedElementAnchor::TopLeft,
                ChildAnchor::BottomLeft,
                ctx,
            );
            dropdown.set_menu_header_text_override(|label| format!("Reasoning: {label}"));
            let items = if reasoning_efforts.is_empty() {
                vec![DropdownItem::new(
                    if connecting {
                        "Loading…"
                    } else {
                        "Not supported"
                    },
                    ProviderAction::ReasoningEffort(String::new()),
                )]
            } else {
                reasoning_efforts
                    .iter()
                    .map(|choice| {
                        DropdownItem::new(
                            choice.label.clone(),
                            ProviderAction::ReasoningEffort(choice.id.clone()),
                        )
                    })
                    .collect()
            };
            dropdown.set_items(items, ctx);
            dropdown.set_selected_to_none(ctx);
            if reasoning_efforts.is_empty() {
                dropdown.set_selected_by_index(0, ctx);
            } else if let Some(effort) = reasoning_effort {
                dropdown.set_selected_by_action(ProviderAction::ReasoningEffort(effort), ctx);
            }
            if busy || reasoning_efforts.is_empty() {
                dropdown.set_disabled(ctx);
            } else {
                dropdown.set_enabled(ctx);
            }
        });
        ctx.notify();
    }
}

impl Entity for ProviderControls {
    type Event = ();
}

impl TypedActionView for ProviderControls {
    type Action = ProviderAction;
    fn handle_action(&mut self, action: &ProviderAction, ctx: &mut ViewContext<Self>) {
        self.error = None;
        match action {
            ProviderAction::AutoApprove(enabled) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyAutoApprove(*enabled));
                return;
            }
            ProviderAction::ApplyAutoApprove(enabled) => {
                if let Some(id) = self.conversation(ctx) {
                    self.error = self
                        .controller
                        .update(ctx, |controller, ctx| {
                            controller.set_provider_auto_approve(id, *enabled, ctx)
                        })
                        .err();
                }
            }
            ProviderAction::Profile(id) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyProfile(id.clone()));
                return;
            }
            ProviderAction::ApplyProfile(id) => {
                AIExecutionProfilesModel::handle(ctx).update(ctx, |profiles, ctx| {
                    profiles.set_active_profile(self.terminal_id, id.clone(), ctx);
                });
                self.queue_profile_model(ctx);
                ctx.dispatch_global_action("workspace:save_app", ());
            }
            ProviderAction::Runtime(runtime) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyRuntime(*runtime));
                return;
            }
            ProviderAction::Model(model) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyModel(model.clone()));
                return;
            }
            ProviderAction::ReasoningEffort(effort) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyReasoningEffort(
                    effort.clone(),
                ));
                return;
            }
            ProviderAction::ApplyRuntime(runtime) => {
                if let Some(id) = self.conversation(ctx) {
                    let result = self.controller.update(ctx, |controller, ctx| {
                        controller.configure_provider_chat(id, *runtime, ctx)
                    });
                    match result {
                        Ok(()) => AISettings::handle(ctx).update(ctx, |settings, ctx| {
                            report_if_error!(settings.native_chat_runtime.set_value(*runtime, ctx));
                        }),
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            ProviderAction::ApplyModel(model) => {
                if let Some(id) = self.conversation(ctx) {
                    self.runtime.update(ctx, |runtime, ctx| {
                        runtime.select_model(id, model.clone(), ctx)
                    });
                }
            }
            ProviderAction::ApplyReasoningEffort(effort) => {
                if let Some(id) = self.conversation(ctx) {
                    self.runtime.update(ctx, |runtime, ctx| {
                        runtime.select_reasoning_effort(id, effort.clone(), ctx)
                    });
                }
            }
            ProviderAction::Answer {
                conversation,
                approval,
                allow,
            } => {
                self.runtime.update(ctx, |runtime, ctx| {
                    runtime.answer(*conversation, *approval, *allow, ctx)
                });
            }
            ProviderAction::Stop(id) => {
                self.runtime.update(ctx, |runtime, ctx| {
                    runtime.cancel(*id, ctx);
                });
            }
        }
        self.refresh(ctx);
    }
}

impl View for ProviderControls {
    fn ui_name() -> &'static str {
        "ProviderChatControls"
    }
    fn render(&self, ctx: &AppContext) -> Box<dyn Element> {
        let Some(id) = self.conversation(ctx) else {
            return Empty::new().finish();
        };
        let appearance = Appearance::as_ref(ctx);
        let mut row = Wrap::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_run_spacing(6.)
            .with_spacing(6.);
        if !warp_core::mywarp::enabled() {
            row.add_child(ChildView::new(&self.runtime_dropdown).finish());
        }
        if !self.is_provider_chat(ctx) {
            return if let Some(error) = &self.error {
                Flex::column()
                    .with_child(row.finish())
                    .with_child(
                        appearance
                            .ui_builder()
                            .paragraph(error.clone())
                            .build()
                            .finish(),
                    )
                    .finish()
            } else {
                row.finish()
            };
        }
        row.add_child(
            SavePosition::new(
                ChildView::new(&self.profile_dropdown).finish(),
                "mywarp-chat-profile",
            )
            .finish(),
        );
        row.add_child(
            SavePosition::new(
                ChildView::new(&self.model_dropdown).finish(),
                "mywarp-chat-model",
            )
            .finish(),
        );
        row.add_child(
            SavePosition::new(
                ChildView::new(&self.reasoning_dropdown).finish(),
                "mywarp-chat-reasoning-effort",
            )
            .finish(),
        );
        row.add_child(
            SavePosition::new(
                ChildView::new(&self.auto_approve_dropdown).finish(),
                "mywarp-chat-auto-approve",
            )
            .finish(),
        );
        if self.runtime.as_ref(ctx).can_stop(id) {
            row.add_child(
                appearance
                    .ui_builder()
                    .button(ButtonVariant::Secondary, self.stop_mouse.clone())
                    .with_text_label("Stop".to_owned())
                    .build()
                    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(ProviderAction::Stop(id)))
                    .finish(),
            );
        }
        let mut column = Flex::column().with_child(row.finish());
        if let Some(session) = self.runtime.as_ref(ctx).session(id) {
            column.add_child(
                appearance
                    .ui_builder()
                    .paragraph(session.status.clone())
                    .build()
                    .finish(),
            );
            if let Some(error) = session.error.as_ref().or(self.error.as_ref()) {
                column.add_child(
                    appearance
                        .ui_builder()
                        .paragraph(error.clone())
                        .build()
                        .finish(),
                );
            }
            if let Some(approval) = session.approvals.first() {
                let approval_id = approval.id;
                column.add_child(
                    appearance
                        .ui_builder()
                        .paragraph(format!("Approval required: {}", approval.title))
                        .build()
                        .finish(),
                );
                let theme = appearance.theme();
                let details = appearance
                    .ui_builder()
                    .paragraph(approval.details.clone())
                    .build()
                    .finish();
                let scroll = ClippedScrollable::vertical(
                    self.approval_scroll.clone(),
                    details,
                    ScrollbarWidth::Auto,
                    theme.nonactive_ui_text_color().into(),
                    theme.active_ui_text_color().into(),
                    warpui::elements::Fill::None,
                )
                .finish();
                column.add_child(ConstrainedBox::new(scroll).with_max_height(160.).finish());
                let mut buttons = Flex::row().with_spacing(6.);
                if approval.can_allow {
                    buttons.add_child(
                        SavePosition::new(
                            appearance
                                .ui_builder()
                                .button(ButtonVariant::Secondary, self.approve_mouse.clone())
                                .with_text_label("Allow once".to_owned())
                                .build()
                                .on_click(move |ctx, _, _| {
                                    ctx.dispatch_typed_action(ProviderAction::Answer {
                                        conversation: id,
                                        approval: approval_id,
                                        allow: true,
                                    })
                                })
                                .finish(),
                            "mywarp-chat-allow-once",
                        )
                        .finish(),
                    );
                }
                buttons.add_child(
                    appearance
                        .ui_builder()
                        .button(ButtonVariant::Secondary, self.deny_mouse.clone())
                        .with_text_label("Deny".to_owned())
                        .build()
                        .on_click(move |ctx, _, _| {
                            ctx.dispatch_typed_action(ProviderAction::Answer {
                                conversation: id,
                                approval: approval_id,
                                allow: false,
                            })
                        })
                        .finish(),
                );
                column.add_child(buttons.finish());
            }
        }
        Container::new(column.finish())
            .with_vertical_padding(4.)
            .finish()
    }
}
