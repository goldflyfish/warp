use serde::Deserialize;

use super::{decode, models_from_snapshot};
use warp_core::provider_setup::{ProviderInfo, Snapshot};

#[test]
fn profile_catalog_uses_connected_provider_models_and_the_configured_default() {
    let snapshot = Snapshot {
        default_model: "custom/second".to_owned(),
        providers: vec![
            ProviderInfo {
                id: "custom".to_owned(),
                name: "Local provider".to_owned(),
                connected: true,
                models: [
                    ("first".to_owned(), "First".to_owned()),
                    ("second".to_owned(), "Second".to_owned()),
                ]
                .into(),
                ..Default::default()
            },
            ProviderInfo {
                id: "disconnected".to_owned(),
                connected: false,
                models: [("other".to_owned(), "Other".to_owned())].into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let models = models_from_snapshot(&snapshot).unwrap();
    let models = serde_json::to_value(models).unwrap();
    assert_eq!(models["agent_mode"]["default_id"], "custom/second");
    assert_eq!(models["agent_mode"]["choices"].as_array().unwrap().len(), 2);
    assert_eq!(models["agent_mode"]["choices"][0]["id"], "custom/first");
    assert_eq!(
        models["agent_mode"]["choices"][1]["display_name"],
        "Second · Local provider"
    );
    assert!(models["agent_mode"]["choices"][1]["disable_reason"].is_null());
}

#[test]
fn profile_catalog_requires_a_connected_provider() {
    assert!(models_from_snapshot(&Snapshot::default()).is_err());
}

#[test]
fn profile_catalog_keeps_identically_named_models_from_different_providers() {
    let snapshot = Snapshot {
        providers: vec![
            ProviderInfo {
                id: "first".to_owned(),
                connected: true,
                models: [("shared".to_owned(), "Shared".to_owned())].into(),
                ..Default::default()
            },
            ProviderInfo {
                id: "second".to_owned(),
                connected: true,
                models: [("shared".to_owned(), "Shared".to_owned())].into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let models = models_from_snapshot(&snapshot).unwrap();
    let models = serde_json::to_value(models).unwrap();
    assert_eq!(
        models["agent_mode"]["choices"][0]["base_model_name"],
        "first/shared"
    );
    assert_eq!(
        models["agent_mode"]["choices"][1]["base_model_name"],
        "second/shared"
    );
}

#[derive(Debug, Deserialize, PartialEq)]
struct Answer {
    title: String,
}

#[test]
fn accepts_plain_or_fenced_structured_provider_output() {
    assert_eq!(
        decode::<Answer>(r#"{"title":"Build fix"}"#).unwrap(),
        Answer {
            title: "Build fix".into()
        }
    );
    assert_eq!(
        decode::<Answer>("```json\n{\"title\":\"Build fix\"}\n```").unwrap(),
        Answer {
            title: "Build fix".into()
        }
    );
}

#[test]
fn rejects_incomplete_or_incorrect_provider_output() {
    assert!(decode::<Answer>("```json\n{\"title\":\"unfinished\"}").is_err());
    assert!(decode::<Answer>("{\"commands\":[]}").is_err());
    assert!(decode::<Answer>("Here is your title: Build fix").is_err());
}
