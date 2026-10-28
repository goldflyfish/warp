use warpui::App;

use super::{AgentsWidget, AllowanceCents, allowance_cents, format_allowance_count};
use crate::settings::UsageDisplayUnit;
use crate::settings_view::settings_page::{FilteredPageType, PageType};

#[test]
fn profile_search_finds_restored_models_and_permissions() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page =
                PageType::new_uncategorized(vec![Box::new(AgentsWidget::default())], None);
            assert!(page.update_filter("full terminal", ctx).is_truthy());
            assert!(page.update_filter("interact running", ctx).is_truthy());
            assert!(page.update_filter("ask questions", ctx).is_truthy());
            assert!(page.update_filter("orchestrated agents", ctx).is_truthy());
            assert!(page.update_filter("mcp denylist", ctx).is_truthy());
            assert!(page.update_filter("web tools", ctx).is_truthy());
            assert!(page.update_filter("sync plans", ctx).is_truthy());
            assert!(!page.update_filter("nonexistent setting", ctx).is_truthy());
            page.update_filter("", ctx);
            let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
                panic!("expected profile widgets");
            };
            assert_eq!(widgets.len(), 1);
        });
    });
}

#[test]
fn allowance_cents_requires_the_dollars_unit_and_both_figures() {
    assert_eq!(
        allowance_cents(UsageDisplayUnit::Dollars, false, Some(70.2), Some(1800.0)),
        Some(AllowanceCents {
            used: 70.2,
            limit: 1800.0,
        })
    );
    assert_eq!(
        allowance_cents(UsageDisplayUnit::Dollars, false, None, Some(1800.0)),
        None
    );
    assert_eq!(
        allowance_cents(UsageDisplayUnit::Dollars, false, Some(70.2), None),
        None
    );
    // Cents the server sends are never shown when displaying in credits.
    assert_eq!(
        allowance_cents(UsageDisplayUnit::Credits, false, Some(70.2), Some(1800.0)),
        None
    );
    // Unlimited subjects keep the credit display.
    assert_eq!(
        allowance_cents(UsageDisplayUnit::Dollars, true, Some(70.2), Some(1800.0)),
        None
    );
}

#[test]
fn allowance_count_shows_dollars_when_the_cents_are_known() {
    let cents = Some(AllowanceCents {
        used: 70.2,
        limit: 1800.0,
    });
    assert_eq!(
        format_allowance_count(39, 1_000, false, cents),
        "$0.70/$18.00"
    );
}

#[test]
fn allowance_count_falls_back_to_credits() {
    assert_eq!(format_allowance_count(39, 1_000, false, None), "39/1000");
}

#[test]
fn allowance_count_keeps_unlimited_display() {
    let cents = Some(AllowanceCents {
        used: 70.2,
        limit: 1800.0,
    });
    assert_eq!(format_allowance_count(39, 999_999, true, None), "Unlimited");
    assert_eq!(
        format_allowance_count(39, 999_999, true, cents),
        "Unlimited"
    );
}
