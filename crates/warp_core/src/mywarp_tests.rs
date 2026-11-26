use super::*;

#[test]
fn blocks_warp_services_and_authentication_origins() {
    assert!(is_warp_service(
        &Url::parse("https://app.warp.dev/login").unwrap()
    ));
    assert!(is_warp_service(
        &Url::parse("wss://rtc.app.warp.dev/graphql/v2").unwrap()
    ));
    assert!(is_warp_service(
        &Url::parse("https://docs.warp.dev./agents").unwrap()
    ));
    assert!(is_warp_service(
        &Url::parse("https://securetoken.googleapis.com/v1/token").unwrap()
    ));
    assert!(is_warp_service(&Url::parse("http://127.0.0.1:0/").unwrap()));
}

#[test]
fn permits_direct_providers_and_local_models() {
    assert!(!is_warp_service(
        &Url::parse("https://openrouter.ai/api/v1").unwrap()
    ));
    assert!(!is_warp_service(
        &Url::parse("https://api.openai.com/v1").unwrap()
    ));
    assert!(!is_warp_service(
        &Url::parse("https://api.anthropic.com/v1").unwrap()
    ));
    assert!(!is_warp_service(
        &Url::parse("https://api.z.ai/api/paas/v4").unwrap()
    ));
    assert!(!is_warp_service(
        &Url::parse("http://127.0.0.1:11434/v1").unwrap()
    ));
    assert!(!is_warp_service(
        &Url::parse("https://warp.dev.example.org/").unwrap()
    ));
}
