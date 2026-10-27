use super::{
    AgentAttributionToggleState, GrokSubscriptionButtonAction,
    derive_agent_attribution_toggle_state, grok_subscription_button_action,
};
use crate::workspaces::workspace::AdminEnablementSetting;

#[test]
fn respect_user_setting_returns_user_pref_unlocked() {
    let state = derive_agent_attribution_toggle_state(
        &AdminEnablementSetting::RespectUserSetting,
        true,
        true,
    );
    assert_eq!(
        state,
        AgentAttributionToggleState {
            is_enabled: true,
            is_forced_by_org: false,
            is_disabled: false,
        }
    );
}

#[test]
fn respect_user_setting_with_user_off_returns_unchecked_unlocked() {
    let state = derive_agent_attribution_toggle_state(
        &AdminEnablementSetting::RespectUserSetting,
        false,
        true,
    );
    assert_eq!(
        state,
        AgentAttributionToggleState {
            is_enabled: false,
            is_forced_by_org: false,
            is_disabled: false,
        }
    );
}

#[test]
fn team_enable_locks_toggle_on_regardless_of_user_pref() {
    let state = derive_agent_attribution_toggle_state(&AdminEnablementSetting::Enable, false, true);
    assert_eq!(
        state,
        AgentAttributionToggleState {
            is_enabled: true,
            is_forced_by_org: true,
            is_disabled: true,
        }
    );
}

#[test]
fn team_disable_locks_toggle_off_regardless_of_user_pref() {
    let state = derive_agent_attribution_toggle_state(&AdminEnablementSetting::Disable, true, true);
    assert_eq!(
        state,
        AgentAttributionToggleState {
            is_enabled: false,
            is_forced_by_org: true,
            is_disabled: true,
        }
    );
}

#[test]
fn ai_globally_disabled_marks_toggle_disabled_but_not_forced() {
    let state = derive_agent_attribution_toggle_state(
        &AdminEnablementSetting::RespectUserSetting,
        true,
        false,
    );
    assert_eq!(
        state,
        AgentAttributionToggleState {
            is_enabled: true,
            is_forced_by_org: false,
            is_disabled: true,
        }
    );
}

#[test]
fn team_force_takes_precedence_over_global_ai_disabled() {
    let state =
        derive_agent_attribution_toggle_state(&AdminEnablementSetting::Enable, false, false);
    assert_eq!(
        state,
        AgentAttributionToggleState {
            is_enabled: true,
            is_forced_by_org: true,
            is_disabled: true,
        }
    );
}

#[test]
fn grok_button_action_reflects_tokens_and_attempt_phase() {
    assert_eq!(
        grok_subscription_button_action(false, None),
        GrokSubscriptionButtonAction::Connect
    );
    assert_eq!(
        grok_subscription_button_action(false, Some(false)),
        GrokSubscriptionButtonAction::Cancel
    );
    assert_eq!(
        grok_subscription_button_action(false, Some(true)),
        GrokSubscriptionButtonAction::Cancelling
    );
    // Stored tokens take precedence regardless of attempt phase.
    assert_eq!(
        grok_subscription_button_action(true, None),
        GrokSubscriptionButtonAction::Disconnect
    );
    assert_eq!(
        grok_subscription_button_action(true, Some(false)),
        GrokSubscriptionButtonAction::Disconnect
    );
    assert_eq!(
        grok_subscription_button_action(true, Some(true)),
        GrokSubscriptionButtonAction::Disconnect
    );
}

#[test]
fn mywarp_provider_settings_search_filters_the_real_widgets() {
    use super::super::settings_page::FilteredPageType;
    warpui::App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = super::mywarp_provider_page();
            for (query, expected) in [
                ("OpenRouter", 1),
                ("permissions", 1),
                ("MCP", 1),
                ("personal instructions", 1),
                ("Sign up", 0),
                ("", 9),
            ] {
                page.update_filter(query, ctx);
                let FilteredPageType::Categorized {
                    categories, title, ..
                } = page.get_filtered()
                else {
                    panic!("expected categorized provider settings")
                };
                let count: usize = categories
                    .iter()
                    .map(|category| category.widgets.len())
                    .sum();
                assert_eq!(count, expected, "query: {query}");
                if expected > 0 {
                    assert!(title.is_some());
                }
            }
        });
    });
}
