use super::{branches_at_commit, checkout_branch_key, nonempty, reset_ref_display};
use crate::model::Branch;

fn branch(name: &str, hash: &str) -> Branch {
    Branch {
        name: name.to_string(),
        hash: hash.to_string(),
        recency: String::new(),
        pushables: String::new(),
        pullables: String::new(),
        upstream: None,
        head: false,
    }
}

#[test]
fn finds_local_branches_at_commit_in_model_order() {
    let branches = vec![
        branch("other", "9999999"),
        branch("feature", "b838172"),
        branch("release", "b838172aabbccdd"),
    ];

    assert_eq!(
        branches_at_commit("b838172aabbccdd", &branches, "main"),
        vec!["feature", "release"]
    );
}

#[test]
fn excludes_the_current_branch_and_empty_hashes() {
    let branches = vec![
        branch("main", "b838172"),
        branch("feature", "b838172"),
        branch("invalid", ""),
    ];

    assert_eq!(
        branches_at_commit("b838172aabbccdd", &branches, "main"),
        vec!["feature"]
    );
}

#[test]
fn assigns_number_shortcuts_to_the_first_nine_branches() {
    assert_eq!(checkout_branch_key(0).as_deref(), Some("1"));
    assert_eq!(checkout_branch_key(8).as_deref(), Some("9"));
    assert_eq!(checkout_branch_key(9), None);
}

#[test]
fn reset_ref_display_shortens_full_hashes_but_keeps_names() {
    assert_eq!(reset_ref_display("b838172aabbccdd1122334455"), "b838172");
    assert_eq!(reset_ref_display("main"), "main");
    assert_eq!(reset_ref_display("v1.2.3"), "v1.2.3");
    // Short hashes and non-hex strings are left as-is.
    assert_eq!(reset_ref_display("b838172"), "b838172");
    assert_eq!(reset_ref_display("feature/foo"), "feature/foo");
}

#[test]
fn trims_nonempty_filter_values() {
    assert_eq!(nonempty("  src/main.rs  "), Some("src/main.rs".to_string()));
}

#[test]
fn treats_blank_filter_values_as_unset() {
    assert_eq!(nonempty("   "), None);
}

#[test]
fn build_branch_checklist_items_starts_with_unchecked_clear_filter() {
    use super::build_branch_checklist_items;

    let branches = vec![branch("main", "1111111"), branch("feature", "2222222")];
    let current_filter = vec!["main".to_string()];
    let items = build_branch_checklist_items(&branches, &current_filter);

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].label, "<Clear Filter>");
    assert!(!items[0].checked);

    assert_eq!(items[1].label, "main");
    assert!(items[1].checked);

    assert_eq!(items[2].label, "feature");
    assert!(!items[2].checked);
}

#[test]
fn resolve_branch_filter_selection_clears_when_clear_filter_selected() {
    use super::resolve_branch_filter_selection;

    let checked = vec!["<Clear Filter>".to_string(), "main".to_string()];
    assert!(resolve_branch_filter_selection(checked).is_empty());

    let checked = vec!["<Clear Filter>".to_string()];
    assert!(resolve_branch_filter_selection(checked).is_empty());
}

#[test]
fn resolve_branch_filter_selection_preserves_selected_branches() {
    use super::resolve_branch_filter_selection;

    let checked = vec!["main".to_string(), "feature".to_string()];
    assert_eq!(
        resolve_branch_filter_selection(checked),
        vec!["main".to_string(), "feature".to_string()]
    );

    let checked: Vec<String> = Vec::new();
    assert!(resolve_branch_filter_selection(checked).is_empty());
}
