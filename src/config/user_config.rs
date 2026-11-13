use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::keybindings::KeybindingConfig;
use super::theme::Theme;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct UserConfig {
    pub gui: GuiConfig,
    pub git: GitConfig,
    pub refresher: RefresherConfig,
    pub keybinding: KeybindingConfig,
    pub os: OsConfig,
    #[serde(rename = "customCommands")]
    pub custom_commands: Vec<CustomCommand>,
    #[serde(rename = "aiNotes")]
    pub ai_notes: AiNotesConfig,
    /// Unknown keys encountered when parsing `config.yml` (reported once in the command log at startup).
    #[serde(skip)]
    pub unknown_keys: Vec<String>,
}

/// Mirrors lazygit's `refresher` config block. `refreshInterval` is the
/// files/submodules auto-refresh cadence; `fetchInterval` is the periodic
/// background `git fetch` cadence. Both are in seconds; 0 disables.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefresherConfig {
    #[serde(rename = "refreshInterval")]
    pub refresh_interval: u64,
    #[serde(rename = "fetchInterval")]
    pub fetch_interval: u64,
}

impl Default for RefresherConfig {
    fn default() -> Self {
        Self {
            refresh_interval: 10,
            fetch_interval: 60,
        }
    }
}

pub(crate) fn expand_tilde(path_str: &str) -> std::path::PathBuf {
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path_str == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    std::path::PathBuf::from(path_str)
}

impl UserConfig {
    pub fn load(config_dir: &Path, config_override: Option<&String>) -> Result<Self> {
        let mut config_path = if config_dir.join("config.yml").exists() {
            config_dir.join("config.yml")
        } else if config_dir.join("config.yaml").exists() {
            config_dir.join("config.yaml")
        } else {
            config_dir.join("config.yml")
        };

        if let Some(over) = config_override {
            let expanded_path = expand_tilde(over);
            if expanded_path.is_absolute()
                || over.ends_with(".yml")
                || over.ends_with(".yaml")
                || expanded_path.exists()
            {
                config_path = expanded_path;
            } else {
                let preset_yaml = config_dir.join("presets").join(format!("{}.yaml", over));
                let preset_yml = config_dir.join("presets").join(format!("{}.yml", over));
                if preset_yaml.exists() {
                    config_path = preset_yaml;
                } else if preset_yml.exists() {
                    config_path = preset_yml;
                } else {
                    anyhow::bail!(
                        "Config file or preset not found: '{}' (checked relative, absolute, and presets in {})",
                        over,
                        config_dir.display()
                    );
                }
            }

            if !config_path.exists() {
                anyhow::bail!("Config file not found at: {}", config_path.display());
            }
        }

        if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)?;
            let mut config: UserConfig = serde_yaml::from_str(&contents)?;
            config.unknown_keys = Self::find_unknown_keys(&contents);
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Compare a raw YAML config string against the canonical serialized keys of `UserConfig::default()`
    /// and return any unrecognized dot-separated paths (e.g. `"gui.borderGradiant"`).
    pub fn find_unknown_keys(yaml_str: &str) -> Vec<String> {
        let Ok(user_val) = serde_yaml::from_str::<serde_json::Value>(yaml_str) else {
            return Vec::new();
        };
        let Ok(schema_val) = serde_json::to_value(UserConfig::default()) else {
            return Vec::new();
        };
        let mut unknown = Vec::new();
        collect_unknown_json_keys("", &user_val, &schema_val, &mut unknown);
        unknown
    }

    pub fn theme(&self) -> Theme {
        Theme::from_config(&self.gui)
    }
}

fn collect_unknown_json_keys(
    prefix: &str,
    user: &serde_json::Value,
    schema: &serde_json::Value,
    out: &mut Vec<String>,
) {
    let serde_json::Value::Object(user_map) = user else {
        return;
    };
    let serde_json::Value::Object(schema_map) = schema else {
        return;
    };
    for (k, v) in user_map {
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        // `gui.border` accepts arbitrary panel keys in Granular mode; `gui.theme` accepts `preset` in Map mode.
        if path == "gui.border" {
            continue;
        }
        if path == "gui.theme" {
            if let serde_json::Value::Object(theme_map) = v {
                for tk in theme_map.keys() {
                    if tk != "preset"
                        && !schema_map
                            .get("theme")
                            .and_then(|t| t.as_object())
                            .is_some_and(|m| m.contains_key(tk))
                    {
                        out.push(format!("gui.theme.{tk}"));
                    }
                }
            }
            continue;
        }
        match schema_map.get(k) {
            Some(schema_child) => {
                if schema_child.is_object() && v.is_object() {
                    collect_unknown_json_keys(&path, v, schema_child, out);
                }
            }
            None => out.push(path),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiConfig {
    #[serde(rename = "scrollHeight")]
    pub scroll_height: usize,
    #[serde(rename = "scrollPastBottom")]
    pub scroll_past_bottom: bool,
    #[serde(rename = "mouseEvents")]
    pub mouse_events: bool,
    #[serde(rename = "skipDiscardChangeWarning")]
    pub skip_discard_change_warning: bool,
    #[serde(rename = "sidePanelWidth")]
    pub side_panel_width: f64,
    pub theme: ThemeConfig,
    #[serde(rename = "showFileTree")]
    pub show_file_tree: bool,
    #[serde(rename = "showCommandLog")]
    pub show_command_log: bool,
    #[serde(rename = "showBottomLine")]
    pub show_bottom_line: bool,
    #[serde(rename = "nerdFontsVersion")]
    pub nerd_fonts_version: String,
    #[serde(default)]
    pub border: BorderConfig,
    /// Gradient for the active border: a preset (aurora, nebula, rainbow,
    /// blackhole, warm, cool, lavender, monochrome) or two or more `#rrggbb`
    /// stops. Empty (the default) keeps the solid active border color.
    #[serde(rename = "borderGradient")]
    pub border_gradient: String,
    /// Draws a rounded frame around the whole app, so it looks like a popup
    /// (e.g. inside a borderless tmux popup). Painted with `borderGradient`
    /// when one is set.
    #[serde(rename = "outerFrame")]
    pub outer_frame: bool,
    #[serde(rename = "sideBySide", default = "default_side_by_side")]
    pub side_by_side: bool,
    #[serde(
        rename = "fileHeaderSeparatorChar",
        default = "default_file_header_separator_char"
    )]
    pub file_header_separator_char: String,
    #[serde(rename = "spinner", default = "default_spinner")]
    pub spinner: Vec<String>,
}

fn default_side_by_side() -> bool {
    true
}

fn default_file_header_separator_char() -> String {
    "─".to_string()
}

fn default_spinner() -> Vec<String> {
    vec![
        "·".to_string(),
        "✻".to_string(),
        "✽".to_string(),
        "✶".to_string(),
        "✳".to_string(),
        "✢".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BorderConfig {
    Global(String),
    Granular(std::collections::HashMap<String, String>),
}

impl Default for BorderConfig {
    fn default() -> Self {
        BorderConfig::Global("rounded".to_string())
    }
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            scroll_height: 2,
            scroll_past_bottom: true,
            mouse_events: true,
            skip_discard_change_warning: false,
            side_panel_width: 0.3333,
            theme: ThemeConfig::default(),
            show_file_tree: true,
            show_command_log: true,
            show_bottom_line: true,
            nerd_fonts_version: "3".to_string(),
            border: BorderConfig::default(),
            border_gradient: String::new(),
            outer_frame: false,
            side_by_side: true,
            file_header_separator_char: "─".to_string(),
            spinner: default_spinner(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ThemeConfig {
    /// Optional theme preset ID (`"omarchy"`, `"cockpit"`, `"catppuccin-mocha"`, etc.).
    /// Can be specified either as `gui:\n  theme: omarchy` or `gui:\n  theme:\n    preset: omarchy`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(rename = "activeBorderColor")]
    pub active_border_color: Vec<String>,
    #[serde(rename = "inactiveBorderColor")]
    pub inactive_border_color: Vec<String>,
    #[serde(rename = "selectedLineBgColor")]
    pub selected_line_bg_color: Vec<String>,
    #[serde(rename = "optionsTextColor")]
    pub options_text_color: Vec<String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            preset: None,
            active_border_color: vec!["green".to_string()],
            inactive_border_color: vec!["default".to_string()],
            selected_line_bg_color: vec!["blue".to_string()],
            options_text_color: vec!["blue".to_string()],
        }
    }
}

impl<'de> Deserialize<'de> for ThemeConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(default)]
        struct ThemeConfigMap {
            preset: Option<String>,
            #[serde(rename = "activeBorderColor")]
            active_border_color: Vec<String>,
            #[serde(rename = "inactiveBorderColor")]
            inactive_border_color: Vec<String>,
            #[serde(rename = "selectedLineBgColor")]
            selected_line_bg_color: Vec<String>,
            #[serde(rename = "optionsTextColor")]
            options_text_color: Vec<String>,
        }

        impl Default for ThemeConfigMap {
            fn default() -> Self {
                let d = ThemeConfig::default();
                Self {
                    preset: d.preset,
                    active_border_color: d.active_border_color,
                    inactive_border_color: d.inactive_border_color,
                    selected_line_bg_color: d.selected_line_bg_color,
                    options_text_color: d.options_text_color,
                }
            }
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ThemeConfigRepr {
            Preset(String),
            Map(ThemeConfigMap),
        }

        match ThemeConfigRepr::deserialize(deserializer)? {
            ThemeConfigRepr::Preset(name) => {
                let trimmed = name.trim().to_string();
                Ok(ThemeConfig {
                    preset: (!trimmed.is_empty()).then_some(trimmed),
                    ..ThemeConfig::default()
                })
            }
            ThemeConfigRepr::Map(m) => Ok(ThemeConfig {
                preset: m.preset,
                active_border_color: m.active_border_color,
                inactive_border_color: m.inactive_border_color,
                selected_line_bg_color: m.selected_line_bg_color,
                options_text_color: m.options_text_color,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    #[serde(rename = "autoFetch")]
    pub auto_fetch: bool,
    #[serde(rename = "autoRefresh")]
    pub auto_refresh: bool,
    #[serde(rename = "branchLogCmd")]
    pub branch_log_cmd: String,
    pub paging: PagingConfig,
    pub commit: CommitConfig,
    pub merging: MergingConfig,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            auto_fetch: true,
            auto_refresh: true,
            branch_log_cmd: "git log --graph --color=always --abbrev-commit --decorate --date=relative --pretty=medium {{branchName}} --".to_string(),
            paging: PagingConfig::default(),
            commit: CommitConfig::default(),
            merging: MergingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct PagingConfig {
    #[serde(rename = "useConfig")]
    pub use_config: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitConfig {
    #[serde(rename = "signOff")]
    pub sign_off: bool,
    #[serde(rename = "autoWrapCommitMessage")]
    pub auto_wrap_commit_message: bool,
    #[serde(rename = "autoWrapWidth")]
    pub auto_wrap_width: usize,
    #[serde(rename = "generateCommand")]
    pub generate_command: String,
    /// Smart multi-commit (`sgc`) planner command. Receives the full prompt
    /// on stdin (`LAZYGITRS_AI_COMMIT_INPUT=prompt`) and prints the model's
    /// JSON answer. Empty falls back to `generateCommand`.
    #[serde(rename = "smartCommitCommand")]
    pub smart_commit_command: String,
    /// Default grouping for smart commit: coarse | medium | fine.
    #[serde(rename = "smartCommitGranularity")]
    pub smart_commit_granularity: String,
}

impl CommitConfig {
    /// The command smart commit should run, falling back to `generateCommand`.
    pub fn smart_commit_command(&self) -> &str {
        let smart = self.smart_commit_command.trim();
        if smart.is_empty() {
            self.generate_command.trim()
        } else {
            smart
        }
    }
}

impl Default for CommitConfig {
    fn default() -> Self {
        Self {
            sign_off: false,
            auto_wrap_commit_message: true,
            auto_wrap_width: 72,
            generate_command: String::new(),
            smart_commit_command: String::new(),
            smart_commit_granularity: "medium".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct MergingConfig {
    #[serde(rename = "manualCommit")]
    pub manual_commit: bool,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OsConfig {
    /// Named editor preset (`helix`, `nvim`, `vim`, `vscode`, `zed`, …).
    /// Fills empty edit templates; ignored when templates are set explicitly.
    #[serde(rename = "editPreset")]
    pub edit_preset: String,
    /// Command template to open a file in the user's editor.
    /// Uses `{{filename}}` as placeholder. e.g. `"hx {{filename}}"`
    pub edit: String,
    /// Command template to open a file at a specific line.
    /// Uses `{{filename}}`, `{{line}}`, and optionally `{{column}}`.
    #[serde(rename = "editAtLine")]
    pub edit_at_line: String,
    /// Command template to open a file at a specific line and wait for close.
    #[serde(rename = "editAtLineAndWait")]
    pub edit_at_line_and_wait: String,
    /// Command template to open a file/URL in the default program.
    /// Uses `{{filename}}` as placeholder.
    pub open: String,
    /// Command template to open a directory in the editor.
    #[serde(rename = "openDirInEditor")]
    pub open_dir_in_editor: String,
    /// Command to copy text to clipboard (text is piped via stdin).
    #[serde(rename = "copyToClipboardCmd")]
    pub copy_to_clipboard_cmd: String,
    /// When true, suspend the TUI and wait for the editor (terminal editors).
    /// When false, spawn and return immediately (GUI editors).
    /// When unset, inferred from `editPreset` / `$EDITOR`.
    #[serde(rename = "editInTerminal")]
    pub edit_in_terminal: Option<bool>,
    /// Shell script of functions/aliases sourced before `:` and custom
    /// commands run (same key as lazygit). `~` is expanded. Empty = none.
    #[serde(rename = "shellFunctionsFile")]
    pub shell_functions_file: String,
}

impl Default for OsConfig {
    fn default() -> Self {
        let (open_cmd, copy_cmd) = if cfg!(target_os = "macos") {
            ("open {{filename}}", "pbcopy")
        } else if cfg!(target_os = "windows") {
            ("start \"\" {{filename}}", "clip")
        } else {
            ("xdg-open {{filename}}", "xclip -selection clipboard")
        };

        Self {
            edit_preset: String::new(),
            edit: String::new(),
            edit_at_line: String::new(),
            edit_at_line_and_wait: String::new(),
            open: open_cmd.to_string(),
            open_dir_in_editor: String::new(),
            copy_to_clipboard_cmd: copy_cmd.to_string(),
            edit_in_terminal: None,
            shell_functions_file: String::new(),
        }
    }
}

/// How an edit/open should be launched from the TUI.
#[derive(Debug, Clone)]
pub struct EditorLaunch {
    pub program: String,
    pub args: Vec<String>,
    /// Leave the alternate screen and wait for the process (hx/nvim/vim).
    pub suspend: bool,
}

impl EditorLaunch {
    pub fn display_cmd(&self) -> String {
        let mut parts = Vec::with_capacity(1 + self.args.len());
        parts.push(self.program.as_str());
        parts.extend(self.args.iter().map(String::as_str));
        parts.join(" ")
    }
}

struct EditPreset {
    edit: &'static str,
    edit_at_line: &'static str,
    open_dir_in_editor: &'static str,
    suspend: bool,
}

fn preset_for(name: &str) -> Option<EditPreset> {
    // Match Go lazygit's editor_presets.go (subset we care about).
    match name {
        "vim" | "nvim" | "vi" => Some(EditPreset {
            edit: "{editor} -- {{filename}}",
            edit_at_line: "{editor} +{{line}} -- {{filename}}",
            open_dir_in_editor: "{editor} -- {{dir}}",
            suspend: true,
        }),
        // Homebrew/cargo install the binary as `hx`. Go lazygit also ships a
        // separate "helix (hx)" preset; we always use `hx` for both names.
        "helix" | "hx" => Some(EditPreset {
            edit: "hx -- {{filename}}",
            edit_at_line: "hx -- {{filename}}:{{line}}:{{column}}",
            open_dir_in_editor: "hx -- {{dir}}",
            suspend: true,
        }),
        "nano" => Some(EditPreset {
            edit: "{editor} -- {{filename}}",
            edit_at_line: "{editor} +{{line}} -- {{filename}}",
            open_dir_in_editor: "{editor} -- {{dir}}",
            suspend: true,
        }),
        "emacs" => Some(EditPreset {
            edit: "emacsclient --alternate-editor=emacs --no-wait -- {{filename}}",
            edit_at_line: "emacsclient --alternate-editor=emacs --no-wait +{{line}} -- {{filename}}",
            open_dir_in_editor: "emacsclient --alternate-editor=emacs --no-wait -- {{dir}}",
            suspend: false,
        }),
        "vscode" | "code" => Some(EditPreset {
            edit: "code --reuse-window -- {{filename}}",
            edit_at_line: "code --reuse-window --goto -- {{filename}}:{{line}}:{{column}}",
            open_dir_in_editor: "code -- {{dir}}",
            suspend: false,
        }),
        "zed" | "zed.exe" => Some(EditPreset {
            edit: "zed -- {{filename}}",
            edit_at_line: "zed -- {{filename}}:{{line}}:{{column}}",
            open_dir_in_editor: "zed -- {{dir}}",
            suspend: false,
        }),
        _ => None,
    }
}

fn guess_editor_name() -> String {
    for key in ["VISUAL", "EDITOR"] {
        if let Ok(val) = std::env::var(key) {
            let trimmed = val.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Take the first token (ignore flags like `nvim -p`).
            let first = trimmed.split_whitespace().next().unwrap_or(trimmed);
            let base = std::path::Path::new(first)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(first);
            return base.to_string();
        }
    }
    String::new()
}

fn resolve_preset(os: &OsConfig) -> (EditPreset, String) {
    let guessed = if os.edit_preset.is_empty() {
        guess_editor_name()
    } else {
        os.edit_preset.clone()
    };
    let editor_bin = if guessed.is_empty() {
        "vim".to_string()
    } else if guessed == "helix" {
        // `editPreset: helix` must not exec a non-existent `helix` binary.
        "hx".to_string()
    } else {
        guessed.clone()
    };
    let preset_name = if os.edit_preset.is_empty() {
        editor_bin.as_str()
    } else {
        os.edit_preset.as_str()
    };
    let preset = preset_for(preset_name).unwrap_or(EditPreset {
        edit: "{editor} -- {{filename}}",
        edit_at_line: "{editor} +{{line}} -- {{filename}}",
        open_dir_in_editor: "{editor} -- {{dir}}",
        suspend: true,
    });
    (preset, editor_bin)
}

fn apply_editor(template: &str, editor: &str) -> String {
    template.replace("{editor}", editor)
}

fn expand_placeholders(
    template: &str,
    filename: &str,
    line: Option<usize>,
    column: Option<usize>,
) -> String {
    let mut s = template
        .replace("{{filename}}", filename)
        .replace("{{dir}}", filename);
    if let Some(ln) = line {
        s = s.replace("{{line}}", &ln.to_string());
    }
    if let Some(col) = column {
        s = s.replace("{{column}}", &col.to_string());
    } else {
        s = s.replace("{{column}}", "1");
    }
    s
}

/// Split a command string into argv. Supports simple double-quoted args.
fn split_command(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in cmd.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

impl OsConfig {
    fn suspend_for_edit(&self, preset_suspend: bool) -> bool {
        self.edit_in_terminal.unwrap_or(preset_suspend)
    }

    fn edit_template(&self) -> (String, bool) {
        let (preset, editor) = resolve_preset(self);
        if !self.edit.is_empty() {
            return (self.edit.clone(), self.suspend_for_edit(preset.suspend));
        }
        (
            apply_editor(preset.edit, &editor),
            self.suspend_for_edit(preset.suspend),
        )
    }

    fn edit_at_line_template(&self) -> (String, bool) {
        let (preset, editor) = resolve_preset(self);
        if !self.edit_at_line.is_empty() {
            return (
                self.edit_at_line.clone(),
                self.suspend_for_edit(preset.suspend),
            );
        }
        (
            apply_editor(preset.edit_at_line, &editor),
            self.suspend_for_edit(preset.suspend),
        )
    }

    fn open_dir_template(&self) -> (String, bool) {
        let (preset, editor) = resolve_preset(self);
        if !self.open_dir_in_editor.is_empty() {
            return (
                self.open_dir_in_editor.clone(),
                self.suspend_for_edit(preset.suspend),
            );
        }
        (
            apply_editor(preset.open_dir_in_editor, &editor),
            self.suspend_for_edit(preset.suspend),
        )
    }

    /// Build a launch plan for editing `filename` (optionally at line/column).
    pub fn plan_edit(
        &self,
        filename: &str,
        line: Option<usize>,
        column: Option<usize>,
    ) -> anyhow::Result<EditorLaunch> {
        let (template, suspend) = if line.is_some() {
            self.edit_at_line_template()
        } else {
            self.edit_template()
        };
        Self::plan_from_template(&template, filename, line, column, suspend)
    }

    /// Build a launch plan for opening a directory in the editor.
    pub fn plan_open_dir(&self, dir: &str) -> anyhow::Result<EditorLaunch> {
        let (template, suspend) = self.open_dir_template();
        Self::plan_from_template(&template, dir, None, None, suspend)
    }

    /// Build a launch plan for `os.open` (may be `hx`/`nvim` or a GUI opener).
    pub fn plan_open(&self, filename: &str) -> anyhow::Result<EditorLaunch> {
        if self.open.is_empty() {
            anyhow::bail!("No open command configured");
        }
        let template = self.open.clone();
        // Heuristic: if the open command looks like a terminal editor, suspend.
        let first = template.split_whitespace().next().unwrap_or("");
        let base = std::path::Path::new(first)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(first);
        let looks_terminal = matches!(
            base,
            "hx" | "helix" | "nvim" | "vim" | "vi" | "nano" | "emacs" | "kak" | "kakoune"
        );
        let suspend = self.edit_in_terminal.unwrap_or(looks_terminal);
        Self::plan_from_template(&template, filename, None, None, suspend)
    }

    fn plan_from_template(
        template: &str,
        filename: &str,
        line: Option<usize>,
        column: Option<usize>,
        suspend: bool,
    ) -> anyhow::Result<EditorLaunch> {
        if template.is_empty() {
            anyhow::bail!("No command configured");
        }
        let cmd_str = expand_placeholders(template, filename, line, column);
        let parts = split_command(&cmd_str);
        if parts.is_empty() {
            anyhow::bail!("Empty command after template expansion");
        }
        Ok(EditorLaunch {
            program: parts[0].clone(),
            args: parts[1..].to_vec(),
            suspend,
        })
    }

    /// Run a command template, replacing `{{filename}}` with the given path.
    /// Spawns without waiting (GUI editors / `open`). Prefer [`plan_edit`] for editors.
    pub fn run_template(template: &str, filename: &str) -> anyhow::Result<()> {
        if template.is_empty() {
            anyhow::bail!("No command configured");
        }
        let cmd_str = expand_placeholders(template, filename, None, None);
        let parts = split_command(&cmd_str);
        if parts.is_empty() {
            anyhow::bail!("Empty command after template expansion");
        }
        crate::os::cmd::log_command(&cmd_str);
        std::process::Command::new(&parts[0])
            .args(&parts[1..])
            .spawn()?;
        Ok(())
    }

    /// Run a command template replacing `{{filename}}`, `{{line}}`, and `{{column}}`.
    pub fn run_template_at_line(
        template: &str,
        filename: &str,
        line: usize,
        column: usize,
    ) -> anyhow::Result<()> {
        if template.is_empty() {
            anyhow::bail!("No command configured");
        }
        let cmd_str = expand_placeholders(template, filename, Some(line), Some(column));
        let parts = split_command(&cmd_str);
        if parts.is_empty() {
            anyhow::bail!("Empty command after template expansion");
        }
        crate::os::cmd::log_command(&cmd_str);
        std::process::Command::new(&parts[0])
            .args(&parts[1..])
            .spawn()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct CustomCommand {
    pub key: String,
    pub context: String,
    pub command: String,
    pub description: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(rename = "showOutput")]
    #[serde(default)]
    pub show_output: bool,
    #[serde(default)]
    pub prompts: Vec<CustomCommandPrompt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCommandPrompt {
    #[serde(rename = "type")]
    pub prompt_type: Option<String>,
    pub title: Option<String>,
    pub key: Option<String>,
    pub command: Option<String>,
    pub filter: Option<String>,
}

/// Configuration for the bidirectional AI review notes workflow.
///
/// When the user presses `S` on a selected note, lazygitrs spawns the
/// `notifyCommand` with `{{session_id}}` and `{{prompt}}` expanded, sending
/// the note context to a running AI CLI session (opencode, codex, gemini, etc).
///
/// The session ID and notify command are registered dynamically by the AI CLI at startup via
/// `POST /session-api {"action":"register","sessionId":"...","notifyCommand":"..."}`.
///
/// Example config.yml:
/// ```yaml
/// aiNotes:
///   enabled: true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiNotesConfig {
    /// Master toggle for the AI notes bidirectional workflow.
    pub enabled: bool,
}

impl Default for AiNotesConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_border_parsing() {
        let yaml = r#"
gui:
  border: "double"
"#;
        let config: UserConfig = serde_yaml::from_str(yaml).unwrap();
        match config.gui.border {
            BorderConfig::Global(b) => assert_eq!(b, "double"),
            _ => panic!("Expected Global"),
        }
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde("~/test/path");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home.join("test/path"));
        }
    }

    #[test]
    fn test_parse_generate_command() {
        let yaml = r#"
git:
  commit:
    generateCommand: "lazycommit commit --message-only"
"#;
        let config: UserConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.git.commit.generate_command,
            "lazycommit commit --message-only"
        );
    }

    #[test]
    fn test_load_nonexistent_override_fails() {
        let temp_dir = std::env::temp_dir();
        let result = UserConfig::load(
            &temp_dir,
            Some(&"/path/that/does/not/exist/definitely/config.yml".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_theme_preset_string_and_map() {
        let yaml_preset = r#"
gui:
  theme: omarchy
"#;
        let config: UserConfig = serde_yaml::from_str(yaml_preset).unwrap();
        assert_eq!(config.gui.theme.preset.as_deref(), Some("omarchy"));

        let yaml_cockpit = r#"
gui:
  theme: cockpit
"#;
        let cockpit_cfg: UserConfig = serde_yaml::from_str(yaml_cockpit).unwrap();
        assert_eq!(cockpit_cfg.gui.theme.preset.as_deref(), Some("cockpit"));
        let theme = cockpit_cfg.theme();
        assert_eq!(theme.selected_bg, ratatui::style::Color::Reset);
        assert!(theme.selection_gutter.is_some());

        let yaml_map = r#"
gui:
  theme:
    activeBorderColor:
      - cyan
      - bold
"#;
        let map_cfg: UserConfig = serde_yaml::from_str(yaml_map).unwrap();
        assert_eq!(map_cfg.gui.theme.preset, None);
        assert_eq!(map_cfg.gui.theme.active_border_color, vec!["cyan", "bold"]);
    }

    #[test]
    fn test_find_unknown_keys_detects_typos_without_failing_load() {
        let yaml = r#"
gui:
  borderGradiant: aurora
  theme: cockpit
git:
  autoFetc: false
"#;
        let unknown = UserConfig::find_unknown_keys(yaml);
        assert_eq!(
            unknown,
            vec!["git.autoFetc".to_string(), "gui.borderGradiant".to_string()]
        );
    }
}
