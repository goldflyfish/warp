use super::*;

#[test]
fn mywarp_keeps_local_ai_and_terminal_features_without_hosted_services() {
    let flags = enabled_features();
    assert!(flags.contains(&FeatureFlag::AgentMode));
    assert!(
        MYWARP_DISABLED_FEATURES
            .iter()
            .all(|flag| !flags.contains(flag))
    );
}
