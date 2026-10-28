use settings::Setting;
use warpui::{App, SingletonEntity};

use super::*;
use crate::settings_view::settings_page::FilteredPageType;
use crate::test_util::settings::initialize_settings_for_tests;

#[test]
fn local_agent_settings_search_separates_runtime_and_connection() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        app.update(|ctx| {
            let mut page = CLIAgentsPageView::build_page();
            page.update_filter("", ctx);
            let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
                panic!("expected CLI agent settings widgets");
            };
            let all_count = widgets.len();
            for (query, expected_id) in [
                (
                    "preferred runtime",
                    LocalAgentRuntimeWidget::static_widget_id(),
                ),
                ("api key", LocalAgentConnectionWidget::static_widget_id()),
                ("Z.ai runtime", LocalAgentRuntimeWidget::static_widget_id()),
                (
                    "Z.ai connection",
                    LocalAgentConnectionWidget::static_widget_id(),
                ),
            ] {
                assert!(page.update_filter(query, ctx).is_truthy());
                let FilteredPageType::Uncategorized { widgets, title, .. } = page.get_filtered()
                else {
                    panic!("expected CLI agent settings widgets");
                };
                assert_eq!(widgets.len(), 1, "search: {query}");
                assert_eq!(widgets[0].widget_id(), expected_id);
                assert!(title.is_some(), "page title should survive filtering");
            }
            page.update_filter("", ctx);
            let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
                panic!("expected CLI agent settings widgets");
            };
            assert_eq!(widgets.len(), all_count);
        });
    });
}

#[test]
fn local_agent_preference_is_local_and_does_not_change_session_mode() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            let session_mode = *settings.default_session_mode_internal.value();
            settings
                .local_agent_runtime
                .set_value(LocalAgentRuntime::Codex, ctx)
                .unwrap();
            assert_eq!(
                *settings.local_agent_runtime.value(),
                LocalAgentRuntime::Codex
            );
            assert_eq!(
                *settings.default_session_mode_internal.value(),
                session_mode
            );
            assert!(matches!(
                PreferredLocalAgentRuntime::sync_to_cloud(),
                settings::SyncToCloud::Never
            ));
        });
    });
}
