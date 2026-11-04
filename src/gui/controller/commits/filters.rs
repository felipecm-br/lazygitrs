//! Commit log filters: branch, path and author filter menus and inputs.

use super::*;

pub fn show_files_filtering_menu(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let selected_path = if gui.show_file_tree {
        gui.file_tree_nodes
            .get(selected)
            .map(|node| node.path.clone())
    } else {
        let model = gui
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        model
            .files
            .get(selected)
            .map(|file| file.current_path().to_string())
    };

    show_file_path_filtering_menu(gui, selected_path)
}

pub fn show_file_path_filtering_menu(gui: &mut Gui, selected_path: Option<String>) -> Result<()> {
    let mut items = Vec::new();
    if let Some(path) = selected_path {
        let label = format!("Filter by path: '{path}'");
        items.push(MenuItem {
            label,
            description: String::new(),
            key: None,
            action: Some(Box::new(move |gui| {
                gui.commit_filter.path = Some(path.clone());
                apply_commit_filters_and_focus(gui)
            })),
        });
    }
    items.push(filter_menu_item(
        &path_filter_menu_label(gui),
        show_path_filter_input,
    ));
    items.push(filter_menu_item(
        &author_filter_menu_label(gui),
        show_author_filter_input,
    ));

    if gui.commit_filter.path.is_some() || !gui.commit_filter.author.is_empty() {
        items.push(filter_menu_item("Reset filters", |gui| {
            gui.commit_filter.path = None;
            gui.commit_filter.author.clear();
            apply_commit_filters_and_focus(gui)
        }));
    }

    gui.popup = PopupState::Menu {
        title: filter_commits_menu_title(gui),
        items,
        selected: 0,
        loading_index: None,
    };
    Ok(())
}

pub fn show_filtering_menu(gui: &mut Gui) -> Result<()> {
    use crate::gui::context::ContextId;

    let active = gui.context_mgr.active();
    let selected = gui.context_mgr.selected_active();
    let (selected_author, selected_branch) = {
        let model = gui
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match active {
            ContextId::Commits => {
                let author = model
                    .commits
                    .get(selected)
                    .map(|commit| format!("{} <{}>", commit.author_name, commit.author_email));
                let branch = model
                    .commits
                    .get(selected)
                    .and_then(|commit| {
                        model
                            .branches
                            .iter()
                            .find(|branch| branch.hash == commit.hash)
                    })
                    .map(|branch| branch.name.clone());
                (author, branch)
            }
            ContextId::BranchCommits => {
                let author = model
                    .sub_commits
                    .get(selected)
                    .map(|commit| format!("{} <{}>", commit.author_name, commit.author_email));
                let branch = matches!(
                    gui.sub_commits_parent_context,
                    ContextId::Branches | ContextId::RemoteBranches
                )
                .then(|| gui.branch_commits_name.clone())
                .filter(|name| !name.is_empty());
                (author, branch)
            }
            ContextId::Branches => (
                None,
                model
                    .branches
                    .get(selected)
                    .map(|branch| branch.name.clone()),
            ),
            _ => (None, None),
        }
    };

    let mut items = Vec::new();
    if let Some(author) = selected_author {
        let label = format!("Filter by author: '{author}'");
        items.push(MenuItem {
            label,
            description: String::new(),
            key: None,
            action: Some(Box::new(move |gui| {
                apply_author_filters(gui, vec![author.clone()])?;
                gui.context_mgr
                    .set_active(crate::gui::context::ContextId::Commits);
                Ok(())
            })),
        });
    }
    if let Some(branch) = selected_branch {
        let label = format!("Filter by branch: '{branch}'");
        items.push(MenuItem {
            label,
            description: String::new(),
            key: None,
            action: Some(Box::new(move |gui| {
                gui.commit_filter.branch = vec![branch.clone()];
                apply_commit_filters_and_focus(gui)
            })),
        });
    }
    items.push(filter_menu_item(
        &path_filter_menu_label(gui),
        show_path_filter_input,
    ));
    items.push(filter_menu_item(
        &branch_filter_menu_label(gui),
        show_branch_filter_menu,
    ));
    items.push(filter_menu_item(
        &author_filter_menu_label(gui),
        show_author_filter_input,
    ));

    if gui.commit_filter.is_active() {
        items.push(filter_menu_item("Reset filters", |gui| {
            gui.commit_filter.clear();
            apply_commit_filters_and_focus(gui)
        }));
    }

    gui.popup = PopupState::Menu {
        title: filter_commits_menu_title(gui),
        items,
        selected: 0,
        loading_index: None,
    };
    Ok(())
}

pub(super) fn show_path_filter_input(gui: &mut Gui) -> Result<()> {
    // Don't block the UI on `git ls-files` / log --name-only — load async.
    if gui.filter_paths_rx.is_some() {
        return Ok(());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    gui.filter_paths_rx = Some(rx);
    gui.popup = PopupState::Message {
        title: "Filter by path".to_string(),
        message: "Loading paths…".to_string(),
        kind: MessageKind::Info,
    };
    let git = Arc::clone(&gui.git);
    std::thread::spawn(move || {
        let _ = tx.send(git.load_commit_filter_paths());
    });
    Ok(())
}

pub(super) fn show_author_filter_input(gui: &mut Gui) -> Result<()> {
    let authors = {
        let model = gui
            .model
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        model
            .commit_filter_authors
            .values()
            .map(|author| format!("{} <{}>", author.name, author.email))
            .collect::<Vec<_>>()
    };
    let mut authors = authors;
    authors.sort_by_key(|author| author.to_lowercase());
    let selected_authors = &gui.commit_filter.author;
    let items = authors
        .into_iter()
        .map(|author| ChecklistItem {
            checked: selected_authors.contains(&author),
            label: author,
            is_free_entry: false,
        })
        .collect();
    gui.popup = PopupState::Checklist {
        title: "Filter by author".to_string(),
        items,
        selected: 0,
        search_textarea: crate::gui::popup::make_checklist_search_textarea(),
        free_entry_category: Some("[author]".to_string()),
        on_confirm: Box::new(apply_author_filters),
    };
    Ok(())
}

pub(super) fn apply_author_filters(gui: &mut Gui, authors: Vec<String>) -> Result<()> {
    gui.commit_filter.author = authors
        .into_iter()
        .map(|author| author.trim().to_string())
        .filter(|author| !author.is_empty())
        .collect();
    apply_commit_filters_and_focus(gui)
}

pub(crate) fn apply_commit_filters_and_focus(gui: &mut Gui) -> Result<()> {
    apply_commit_filters(gui)?;
    gui.context_mgr
        .set_active(crate::gui::context::ContextId::Commits);
    Ok(())
}

pub(crate) fn nonempty_for_filter(value: &str) -> Option<String> {
    nonempty(value)
}

pub(super) fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub(super) fn filter_menu_item(label: &str, action: fn(&mut Gui) -> Result<()>) -> MenuItem {
    MenuItem {
        label: label.to_string(),
        description: String::new(),
        key: None,
        action: Some(Box::new(action)),
    }
}

pub(super) fn active_commit_filter_summary(gui: &Gui) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(path) = gui
        .commit_filter
        .path
        .as_deref()
        .filter(|path| !path.is_empty())
    {
        parts.push(format!("path: {path}"));
    }
    if !gui.commit_filter.author.is_empty() {
        parts.push(format!("author: {}", gui.commit_filter.author.join(", ")));
    }
    if !gui.commit_filter.branch.is_empty() {
        parts.push(format!("branch: {}", gui.commit_filter.branch.join(", ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" | "))
    }
}

pub(super) fn filter_commits_menu_title(gui: &Gui) -> String {
    match active_commit_filter_summary(gui) {
        Some(summary) => format!("Filter commits ({summary})"),
        None => "Filter commits".to_string(),
    }
}

pub(super) fn path_filter_menu_label(gui: &Gui) -> String {
    match gui.commit_filter.path.as_deref() {
        Some(path) if !path.is_empty() => format!("Enter path to filter by: '{path}'"),
        _ => "Enter path to filter by".to_string(),
    }
}

pub(super) fn author_filter_menu_label(gui: &Gui) -> String {
    if gui.commit_filter.author.is_empty() {
        "Enter author to filter by".to_string()
    } else {
        format!(
            "Enter author to filter by: '{}'",
            gui.commit_filter.author.join(", ")
        )
    }
}

pub(super) fn branch_filter_menu_label(gui: &Gui) -> String {
    if gui.commit_filter.branch.is_empty() {
        "Enter branches to filter by".to_string()
    } else {
        format!(
            "Enter branches to filter by: '{}'",
            gui.commit_filter.branch.join(", ")
        )
    }
}

pub(super) fn apply_commit_filters(gui: &mut Gui) -> Result<()> {
    // Scoped async commit reload — matches lazygit (no full model refresh).
    gui.reload_filtered_commits_async();
    Ok(())
}

pub(super) fn build_branch_checklist_items(
    branches: &[Branch],
    current_filter: &[String],
) -> Vec<ChecklistItem> {
    let mut items = vec![ChecklistItem {
        label: "<Clear Filter>".to_string(),
        checked: false,
        is_free_entry: false,
    }];

    for b in branches {
        let checked = current_filter.contains(&b.name);
        items.push(ChecklistItem {
            label: b.name.clone(),
            checked,
            is_free_entry: false,
        });
    }

    items
}

pub(super) fn resolve_branch_filter_selection(checked: Vec<String>) -> Vec<String> {
    if checked.iter().any(|val| val == "<Clear Filter>") {
        Vec::new()
    } else {
        checked
    }
}

pub(super) fn show_branch_filter_menu(gui: &mut Gui) -> Result<()> {
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let items = build_branch_checklist_items(&model.branches, &gui.commit_filter.branch);
    drop(model);

    gui.popup = PopupState::Checklist {
        title: "Filter commits by branch".to_string(),
        items,
        selected: 0,
        search_textarea: crate::gui::popup::make_checklist_search_textarea(),
        on_confirm: Box::new(|gui: &mut Gui, checked: Vec<String>| {
            gui.commit_filter.branch = resolve_branch_filter_selection(checked);
            apply_commit_filters_and_focus(gui)
        }),
        free_entry_category: None,
    };

    Ok(())
}
