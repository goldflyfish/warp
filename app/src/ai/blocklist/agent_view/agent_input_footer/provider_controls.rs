use settings::Setting as _;
use uuid::Uuid;
use warp_errors::report_if_error;
use warpui::elements::{
    ChildAnchor, ChildView, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container,
    CrossAxisAlignment, Empty, Flex, MainAxisSize, MouseStateHandle, ParentElement,
    PositionedElementAnchor, ScrollbarWidth,
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
use crate::appearance::Appearance;
use crate::settings::{AISettings, NativeChatRuntime};
use crate::view_components::{Dropdown, DropdownItem};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ProviderAction {
    Runtime(NativeChatRuntime),
    Model(String),
    ApplyRuntime(NativeChatRuntime),
    ApplyModel(String),
    Answer {
        conversation: AIConversationId,
        approval: Uuid,
        allow: bool,
    },
    Stop(AIConversationId),
    Retry(AIConversationId),
    Setup,
}

pub(super) struct ProviderControls {
    terminal_id: EntityId,
    controller: ModelHandle<BlocklistAIController>,
    runtime: ModelHandle<DesktopAcp>,
    runtime_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    model_dropdown: ViewHandle<Dropdown<ProviderAction>>,
    approve_mouse: MouseStateHandle,
    deny_mouse: MouseStateHandle,
    stop_mouse: MouseStateHandle,
    retry_mouse: MouseStateHandle,
    setup_mouse: MouseStateHandle,
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
            approve_mouse: Default::default(),
            deny_mouse: Default::default(),
            stop_mouse: Default::default(),
            retry_mouse: Default::default(),
            setup_mouse: Default::default(),
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

    fn refresh(&mut self, ctx: &mut ViewContext<Self>) {
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
            .map(|session| session.models.clone())
            .unwrap_or_default();
        let selected = session.and_then(|session| session.model_id.clone());
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
            if let Some(selected) = selected {
                dropdown.set_selected_by_action(ProviderAction::Model(selected), ctx);
            }
            if busy || choices.is_empty() {
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
            ProviderAction::Runtime(runtime) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyRuntime(*runtime));
                return;
            }
            ProviderAction::Model(model) => {
                ctx.dispatch_typed_action_deferred(ProviderAction::ApplyModel(model.clone()));
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
            ProviderAction::Retry(id) => {
                self.controller.update(ctx, |controller, ctx| {
                    controller.reconnect_provider_chat(*id, ctx)
                });
            }
            ProviderAction::Setup => {
                ctx.dispatch_typed_action(
                    &crate::workspace::WorkspaceAction::ShowSettingsPageWithSearch {
                        search_query: String::new(),
                        section: Some(crate::settings_view::SettingsSection::WarpAgent),
                    },
                );
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
        let mut row = Flex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.)
            .with_child(ChildView::new(&self.runtime_dropdown).finish());
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
        row.add_child(ChildView::new(&self.model_dropdown).finish());
        row.add_child(
            warpui::elements::SavePosition::new(
                appearance
                    .ui_builder()
                    .button(ButtonVariant::Secondary, self.setup_mouse.clone())
                    .with_text_label("Providers".to_owned())
                    .build()
                    .on_click(|ctx, _, _| ctx.dispatch_typed_action(ProviderAction::Setup))
                    .finish(),
                "mywarp-chat-providers",
            )
            .finish(),
        );
        if !self.runtime.as_ref(ctx).can_stop(id) {
            row.add_child(
                appearance
                    .ui_builder()
                    .button(ButtonVariant::Secondary, self.retry_mouse.clone())
                    .with_text_label("Reconnect".to_owned())
                    .build()
                    .on_click(move |ctx, _, _| ctx.dispatch_typed_action(ProviderAction::Retry(id)))
                    .finish(),
            );
        }
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
