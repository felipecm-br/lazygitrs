//! Copy commit data to the clipboard and open commits in the browser.

use super::*;

pub(super) fn open_commit_in_browser_menu(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        drop(model);
        open_commit_in_browser_menu_for(gui, hash);
    }
    Ok(())
}

pub fn open_commit_in_browser_menu_for(gui: &mut Gui, hash: String) {
    gui.popup = PopupState::Menu {
        title: "Open in browser".to_string(),
        items: vec![MenuItem {
            label: "Open commit URL".to_string(),
            description: String::new(),
            key: Some("c".to_string()),
            action: Some(Box::new(move |gui| {
                let url = gui.git.get_commit_url(&hash)?;
                Platform::open_file(&url)?;
                Ok(())
            })),
        }],
        selected: 0,
        loading_index: None,
    };
}

pub(super) fn copy_to_clipboard_menu(gui: &mut Gui) -> Result<()> {
    let selected = gui.context_mgr.selected_active();
    let model = gui
        .model
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(commit) = model.commits.get(selected) {
        let hash = commit.hash.clone();
        let subject = commit.name.clone();
        let author = commit.author_name.clone();
        let tags = commit.tags.clone();
        drop(model);
        copy_commit_to_clipboard_menu_for(gui, hash, subject, author, tags);
    }
    Ok(())
}

pub fn copy_commit_to_clipboard_menu_for(
    gui: &mut Gui,
    hash: String,
    subject: String,
    author: String,
    tag_list: Vec<String>,
) {
    {
        let tags = tag_list.join(", ");
        let has_tags = !tag_list.is_empty();
        let hash_for_url = hash.clone();
        let hash_for_msg = hash.clone();
        let hash_for_body = hash.clone();
        let hash_for_diff = hash.clone();

        // Check if commit has a body (for strikethrough on empty)
        let has_body = gui
            .git
            .commit_message_body(&hash)
            .map(|b| !b.trim().is_empty())
            .unwrap_or(false);

        gui.popup = PopupState::Menu {
            title: "Copy to clipboard".to_string(),
            items: vec![
                MenuItem {
                    label: "Commit hash".to_string(),
                    description: String::new(),
                    key: None,
                    action: Some(Box::new(move |_gui| {
                        Platform::copy_to_clipboard(&hash)?;
                        Ok(())
                    })),
                },
                MenuItem {
                    label: "Commit subject".to_string(),
                    description: String::new(),
                    key: Some("s".to_string()),
                    action: Some(Box::new(move |_gui| {
                        Platform::copy_to_clipboard(&subject)?;
                        Ok(())
                    })),
                },
                MenuItem {
                    label: "Commit message (subject and body)".to_string(),
                    description: String::new(),
                    key: Some("m".to_string()),
                    action: Some(Box::new(move |gui| {
                        let msg = gui.git.commit_message_full(&hash_for_msg)?;
                        Platform::copy_to_clipboard(&msg)?;
                        Ok(())
                    })),
                },
                MenuItem {
                    label: "Commit message body".to_string(),
                    description: if has_body {
                        String::new()
                    } else {
                        "Commit has no message body".to_string()
                    },
                    key: Some("b".to_string()),
                    action: if has_body {
                        Some(Box::new(move |gui| {
                            let body = gui.git.commit_message_body(&hash_for_body)?;
                            Platform::copy_to_clipboard(&body)?;
                            Ok(())
                        }))
                    } else {
                        None
                    },
                },
                MenuItem {
                    label: "Commit URL".to_string(),
                    description: String::new(),
                    key: Some("u".to_string()),
                    action: Some(Box::new(move |gui| {
                        if let Ok(url) = gui.git.get_commit_url(&hash_for_url) {
                            Platform::copy_to_clipboard(&url)?;
                        }
                        Ok(())
                    })),
                },
                MenuItem {
                    label: "Commit diff".to_string(),
                    description: String::new(),
                    key: Some("d".to_string()),
                    action: Some(Box::new(move |gui| {
                        let diff = gui.git.commit_diff(&hash_for_diff)?;
                        Platform::copy_to_clipboard(&diff)?;
                        Ok(())
                    })),
                },
                MenuItem {
                    label: "Commit author".to_string(),
                    description: String::new(),
                    key: Some("a".to_string()),
                    action: Some(Box::new(move |_gui| {
                        Platform::copy_to_clipboard(&author)?;
                        Ok(())
                    })),
                },
                MenuItem {
                    label: "Commit tags".to_string(),
                    description: if has_tags {
                        String::new()
                    } else {
                        "Commit has no tags".to_string()
                    },
                    key: Some("t".to_string()),
                    action: if has_tags {
                        Some(Box::new(move |_gui| {
                            Platform::copy_to_clipboard(&tags)?;
                            Ok(())
                        }))
                    } else {
                        None
                    },
                },
                MenuItem {
                    label: "Cancel".to_string(),
                    description: String::new(),
                    key: None,
                    action: Some(Box::new(|_| Ok(()))),
                },
            ],
            selected: 0,
            loading_index: None,
        };
    }
}
