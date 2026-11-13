//! Smart Commit (`sgc`) in the TUI: background planning, the review popup
//! (toggle, reorder, merge, edit, preview each group's diff) and sequential
//! commit execution. The planner itself lives in `git::smart_commit`.

use std::sync::{Arc, Mutex};

use ratatui_textarea::TextArea;

use super::*;
use crate::git::smart_commit::{
    self as sc, Granularity, SmartCommitGroup, SmartCommitOptions, SmartCommitPlan,
};

pub(crate) struct SmartCommitJob {
    generation: u64,
    cancel: Arc<AtomicBool>,
    cancel_armed_at: Option<Instant>,
    /// Latest progress line from the worker, shown in the loading overlay.
    progress: Arc<Mutex<String>>,
}

pub(crate) enum SmartCommitMsg {
    Planned {
        generation: u64,
        options: SmartCommitOptions,
        result: Result<Option<SmartCommitPlan>>,
    },
    Committed {
        generation: u64,
        created: Vec<(String, String)>,
        failure: Option<(String, String)>,
        remaining: usize,
    },
}

/// Review state for [`PopupState::SmartCommitPlan`].
pub struct SmartCommitState {
    pub plan: SmartCommitPlan,
    pub checked: Vec<bool>,
    pub selected: usize,
    pub options: SmartCommitOptions,
    /// Cached full diff of the selected group: (files it was built for, lines).
    pub preview: Option<(Vec<String>, Vec<String>)>,
    pub preview_scroll: usize,
    /// Inline editor for the selected group's message.
    pub editor: Option<TextArea<'static>>,
}

impl SmartCommitState {
    fn new(plan: SmartCommitPlan, options: SmartCommitOptions) -> Self {
        let checked = vec![true; plan.groups.len()];
        Self {
            plan,
            checked,
            selected: 0,
            options,
            preview: None,
            preview_scroll: 0,
            editor: None,
        }
    }

    pub fn checked_count(&self) -> usize {
        self.checked.iter().filter(|c| **c).count()
    }

    fn select(&mut self, index: usize) {
        if self.plan.groups.is_empty() {
            return;
        }
        let index = index.min(self.plan.groups.len() - 1);
        if index != self.selected {
            self.selected = index;
            self.preview_scroll = 0;
        }
    }

    fn swap(&mut self, a: usize, b: usize) {
        self.plan.groups.swap(a, b);
        self.checked.swap(a, b);
    }

    /// Fold the selected group into the next one (files + body).
    fn merge_with_next(&mut self) {
        let i = self.selected;
        if i + 1 >= self.plan.groups.len() {
            return;
        }
        let next = self.plan.groups.remove(i + 1);
        self.checked.remove(i + 1);
        let group = &mut self.plan.groups[i];
        group.files.extend(next.files);
        let extra = [next.title.as_str(), next.body.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s.starts_with('-') {
                    s.to_string()
                } else {
                    format!("- {s}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !extra.is_empty() {
            group.body = [group.body.trim(), extra.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
        }
        group.fallback = false;
        self.preview = None;
    }
}

impl Gui {
    pub(crate) fn smart_commit_active(&self) -> bool {
        self.smart_commit_job.is_some()
    }

    /// Title and message for the loading overlay while a job runs.
    pub(crate) fn busy_overlay(&self) -> Option<(&'static str, String, bool)> {
        if self.ai_commit_generation_active() {
            return Some((
                "AI Commit",
                "Generating commit message...".to_string(),
                true,
            ));
        }
        if let Some(job) = &self.smart_commit_job {
            let progress = job.progress.lock().map(|p| p.clone()).unwrap_or_default();
            return Some(("Smart Commit", progress, true));
        }
        None
    }

    fn smart_commit_default_options(&self) -> SmartCommitOptions {
        SmartCommitOptions {
            granularity: Granularity::parse(
                &self.config.user_config.git.commit.smart_commit_granularity,
            )
            .unwrap_or_default(),
            ..Default::default()
        }
    }

    /// Files-panel entry point.
    pub fn open_smart_commit(&mut self) -> Result<()> {
        let options = self.smart_commit_default_options();
        self.start_smart_commit_plan(options);
        Ok(())
    }

    pub(crate) fn start_smart_commit_plan(&mut self, options: SmartCommitOptions) {
        if self.smart_commit_active() || self.ai_commit_generation_active() {
            return;
        }
        let command = self
            .config
            .user_config
            .git
            .commit
            .smart_commit_command()
            .to_string();
        if command.is_empty() {
            self.popup = PopupState::Message {
                title: "Smart commit unavailable".to_string(),
                message: "Set git.commit.smartCommitCommand (or generateCommand) to a command \
                          that reads a prompt on stdin and prints the model's answer."
                    .to_string(),
                kind: MessageKind::Error,
            };
            return;
        }

        self.popup = PopupState::None;
        let job = self.new_smart_commit_job("Reading working tree");
        let (generation, cancel, progress) = (
            job.generation,
            Arc::clone(&job.cancel),
            Arc::clone(&job.progress),
        );
        self.smart_commit_job = Some(job);
        let repo = self.git.repo_path().to_path_buf();
        let tx = self.smart_commit_tx.clone();
        std::thread::spawn(move || {
            let report = move |line: &str| {
                if let Ok(mut p) = progress.lock() {
                    *p = line.to_string();
                }
            };
            let result = sc::plan(&repo, &command, &options, cancel, &report);
            let _ = tx.send(SmartCommitMsg::Planned {
                generation,
                options,
                result,
            });
        });
    }

    fn new_smart_commit_job(&mut self, initial: &str) -> SmartCommitJob {
        self.smart_commit_generation = self.smart_commit_generation.wrapping_add(1);
        SmartCommitJob {
            generation: self.smart_commit_generation,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_armed_at: None,
            progress: Arc::new(Mutex::new(initial.to_string())),
        }
    }

    pub(super) fn receive_smart_commit_results(&mut self) {
        while let Ok(msg) = self.smart_commit_rx.try_recv() {
            let generation = match &msg {
                SmartCommitMsg::Planned { generation, .. }
                | SmartCommitMsg::Committed { generation, .. } => *generation,
            };
            if self.smart_commit_job.as_ref().map(|j| j.generation) != Some(generation) {
                continue;
            }
            self.smart_commit_job = None;
            match msg {
                SmartCommitMsg::Planned {
                    options, result, ..
                } => match result {
                    Ok(Some(plan)) => {
                        let mut state = SmartCommitState::new(plan, options);
                        self.load_smart_commit_preview(&mut state);
                        self.popup = PopupState::SmartCommitPlan(Box::new(state));
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.popup = PopupState::Message {
                            title: "Smart commit failed".to_string(),
                            message: format!("{e:#}"),
                            kind: MessageKind::Error,
                        };
                    }
                },
                SmartCommitMsg::Committed {
                    created,
                    failure,
                    remaining,
                    ..
                } => {
                    self.needs_refresh = true;
                    let mut message: String = created
                        .iter()
                        .map(|(hash, title)| format!("{hash}  {title}\n"))
                        .collect();
                    if remaining > 0 {
                        message.push_str(&format!(
                            "\n{remaining} group(s) kept for the next smart commit."
                        ));
                    }
                    self.popup = match failure {
                        Some((title, error)) => PopupState::Message {
                            title: "Smart commit stopped".to_string(),
                            message: format!(
                                "{}Failed on '{title}':\n{error}\n{message}",
                                if created.is_empty() {
                                    String::new()
                                } else {
                                    format!(
                                        "Created {} commit(s) before the failure.\n",
                                        created.len()
                                    )
                                }
                            ),
                            kind: MessageKind::Error,
                        },
                        None => PopupState::Message {
                            title: format!("Created {} commit(s)", created.len()),
                            message: message.trim_end().to_string(),
                            kind: MessageKind::Info,
                        },
                    };
                }
            }
        }
    }

    /// Double-Esc cancels a running plan, like AI commit generation.
    pub(super) fn handle_smart_commit_cancel_key(&mut self, key: KeyEvent) -> bool {
        if key.code != KeyCode::Esc {
            return false;
        }
        let Some(job) = &mut self.smart_commit_job else {
            return false;
        };
        let now = Instant::now();
        let armed = job
            .cancel_armed_at
            .is_some_and(|at| now.duration_since(at) <= Duration::from_millis(900));
        if armed {
            job.cancel.store(true, Ordering::Relaxed);
            self.smart_commit_job = None;
        } else {
            job.cancel_armed_at = Some(now);
        }
        true
    }

    fn load_smart_commit_preview(&self, state: &mut SmartCommitState) {
        let Some(group) = state.plan.groups.get(state.selected) else {
            state.preview = None;
            return;
        };
        if state
            .preview
            .as_ref()
            .is_some_and(|(files, _)| *files == group.files)
        {
            return;
        }
        let lines = match sc::group_preview(self.git.repo_path(), group, &state.plan.files) {
            Ok(text) => text.lines().map(String::from).collect(),
            Err(e) => vec![format!("(preview unavailable: {e})")],
        };
        state.preview = Some((group.files.clone(), lines));
    }

    pub(super) fn handle_smart_commit_plan_key(&mut self, key: KeyEvent) -> Result<()> {
        let PopupState::SmartCommitPlan(state) = &mut self.popup else {
            return Ok(());
        };

        if let Some(editor) = state.editor.as_mut() {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            match key.code {
                KeyCode::Esc => state.editor = None,
                KeyCode::Char('s') if ctrl => save_editor(state),
                KeyCode::Enter if alt || ctrl => save_editor(state),
                KeyCode::Enter => editor.insert_newline(),
                _ => {
                    textarea_input(editor, key);
                }
            }
            return Ok(());
        }

        let count = state.plan.groups.len();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = (self.layout.height / 3).max(1) as usize;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                // Keep edits/order for next time; the tree is unchanged.
                let state = match std::mem::replace(&mut self.popup, PopupState::None) {
                    PopupState::SmartCommitPlan(state) => state,
                    _ => return Ok(()),
                };
                self.persist_smart_commit_groups(&state.options, state.plan.groups.clone());
                return Ok(());
            }
            KeyCode::Down | KeyCode::Char('j') => state.select(state.selected + 1),
            KeyCode::Up | KeyCode::Char('k') => state.select(state.selected.saturating_sub(1)),
            KeyCode::Home => state.select(0),
            KeyCode::End => state.select(count.saturating_sub(1)),
            KeyCode::Char('J') if state.selected + 1 < count => {
                state.swap(state.selected, state.selected + 1);
                state.selected += 1;
            }
            KeyCode::Char('K') if state.selected > 0 => {
                state.swap(state.selected, state.selected - 1);
                state.selected -= 1;
            }
            KeyCode::Char(' ') => {
                if let Some(c) = state.checked.get_mut(state.selected) {
                    *c = !*c;
                }
            }
            KeyCode::Char('a') => {
                let all = state.checked.iter().all(|c| *c);
                state.checked.iter_mut().for_each(|c| *c = !all);
            }
            KeyCode::Char('m') => state.merge_with_next(),
            KeyCode::Char('e') => {
                if let Some(group) = state.plan.groups.get(state.selected) {
                    let mut editor = TextArea::new(
                        group
                            .message()
                            .lines()
                            .map(String::from)
                            .collect::<Vec<_>>(),
                    );
                    editor.set_cursor_line_style(ratatui::style::Style::default());
                    editor.move_cursor(ratatui_textarea::CursorMove::End);
                    state.editor = Some(editor);
                }
            }
            KeyCode::PageDown => state.preview_scroll += page,
            KeyCode::Char('d') if ctrl => state.preview_scroll += page,
            KeyCode::PageUp => state.preview_scroll = state.preview_scroll.saturating_sub(page),
            KeyCode::Char('u') if ctrl => {
                state.preview_scroll = state.preview_scroll.saturating_sub(page)
            }
            KeyCode::Char('r') => {
                let options = SmartCommitOptions {
                    force: true,
                    ..state.options.clone()
                };
                self.start_smart_commit_plan(options);
                return Ok(());
            }
            KeyCode::Char('g') => {
                let options = SmartCommitOptions {
                    force: true,
                    granularity: state.options.granularity.next(),
                    ..state.options.clone()
                };
                self.start_smart_commit_plan(options);
                return Ok(());
            }
            KeyCode::Enter => return self.execute_smart_commit_plan(),
            _ => {}
        }

        if let PopupState::SmartCommitPlan(mut state) =
            std::mem::replace(&mut self.popup, PopupState::None)
        {
            self.load_smart_commit_preview(&mut state);
            self.popup = PopupState::SmartCommitPlan(state);
        }
        Ok(())
    }

    fn persist_smart_commit_groups(
        &self,
        options: &SmartCommitOptions,
        groups: Vec<SmartCommitGroup>,
    ) {
        let repo = self.git.repo_path().to_path_buf();
        let command = self
            .config
            .user_config
            .git
            .commit
            .smart_commit_command()
            .to_string();
        let options = options.clone();
        std::thread::spawn(move || {
            let _ = sc::save_remaining(&repo, &command, &options, &groups);
        });
    }

    fn execute_smart_commit_plan(&mut self) -> Result<()> {
        let PopupState::SmartCommitPlan(state) = &self.popup else {
            return Ok(());
        };
        if state.checked_count() == 0 {
            return Ok(());
        }
        let PopupState::SmartCommitPlan(state) =
            std::mem::replace(&mut self.popup, PopupState::None)
        else {
            return Ok(());
        };
        let state = *state;
        let (accepted, mut rest): (Vec<_>, Vec<_>) = state
            .plan
            .groups
            .into_iter()
            .zip(state.checked)
            .partition(|(_, checked)| *checked);
        let accepted: Vec<SmartCommitGroup> = accepted.into_iter().map(|(g, _)| g).collect();
        let files = state.plan.files;
        let options = state.options;
        let sign_off = self.config.user_config.git.commit.sign_off;
        let command = self
            .config
            .user_config
            .git
            .commit
            .smart_commit_command()
            .to_string();

        let total = accepted.len();
        let job = self.new_smart_commit_job(&format!("Committing 1/{total}"));
        let (generation, progress) = (job.generation, Arc::clone(&job.progress));
        self.smart_commit_job = Some(job);
        let repo = self.git.repo_path().to_path_buf();
        let tx = self.smart_commit_tx.clone();
        std::thread::spawn(move || {
            let mut created = Vec::new();
            let mut failure = None;
            let mut leftovers: Vec<SmartCommitGroup> = Vec::new();
            for (i, group) in accepted.into_iter().enumerate() {
                if failure.is_some() {
                    leftovers.push(group);
                    continue;
                }
                if let Ok(mut p) = progress.lock() {
                    *p = format!("Committing {}/{total}: {}", i + 1, group.title);
                }
                match sc::commit_group(&repo, &group, &files, sign_off) {
                    Ok(hash) => created.push((hash, group.title.clone())),
                    Err(e) => {
                        failure = Some((group.title.clone(), format!("{e:#}")));
                        leftovers.push(group);
                    }
                }
            }
            leftovers.extend(rest.drain(..).map(|(g, _)| g));
            let remaining = leftovers.len();
            let _ = sc::save_remaining(&repo, &command, &options, &leftovers);
            let _ = tx.send(SmartCommitMsg::Committed {
                generation,
                created,
                failure,
                remaining,
            });
        });
        Ok(())
    }
}

fn save_editor(state: &mut SmartCommitState) {
    let Some(editor) = state.editor.take() else {
        return;
    };
    let text = editor.lines().join("\n");
    if text.trim().is_empty() {
        return;
    }
    if let Some(group) = state.plan.groups.get_mut(state.selected) {
        group.set_message(&text);
    }
}
