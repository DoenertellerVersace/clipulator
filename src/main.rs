use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;

use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use gdk::Display;
use gio::prelude::*;
use glib::{Continue, MainContext, PRIORITY_DEFAULT};
use gtk::prelude::*;
use once_cell::sync::Lazy;
use serde::Deserialize;

const DEFAULT_HISTORY_LIMIT: usize = 25;
static DEFAULT_CONFIG: Lazy<AppConfig> = Lazy::new(AppConfig::default);

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct AppConfig {
    history_size: usize,
    commands: Vec<CommandConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            history_size: DEFAULT_HISTORY_LIMIT,
            commands: vec![
                CommandConfig {
                    id: "uppercase".into(),
                    name: "Uppercase".into(),
                    accelerator: Some("<Primary><Shift>U".into()),
                    builtin: Some(BuiltinCommand::Uppercase),
                    ..Default::default()
                },
                CommandConfig {
                    id: "lowercase".into(),
                    name: "Lowercase".into(),
                    accelerator: Some("<Primary><Shift>L".into()),
                    builtin: Some(BuiltinCommand::Lowercase),
                    ..Default::default()
                },
                CommandConfig {
                    id: "trim".into(),
                    name: "Trim whitespace".into(),
                    accelerator: Some("<Primary><Shift>T".into()),
                    builtin: Some(BuiltinCommand::Trim),
                    ..Default::default()
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct CommandConfig {
    id: String,
    name: String,
    accelerator: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    builtin: Option<BuiltinCommand>,
    #[serde(default)]
    exec: Option<Vec<String>>,
    #[serde(default)]
    paste: bool,
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            accelerator: None,
            description: None,
            builtin: None,
            exec: None,
            paste: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BuiltinCommand {
    Uppercase,
    Lowercase,
    Trim,
    Titlecase,
}

#[derive(Debug)]
enum CommandOutcome {
    Completed { output: String },
    Empty,
    Failed(String),
}

fn main() {
    let application =
        gtk::Application::new(Some("dev.clipulator.App"), gio::ApplicationFlags::empty())
            .expect("Failed to initialise GTK application");

    application.connect_startup(|app| {
        if let Err(err) = ensure_default_config() {
            eprintln!("Failed to prepare configuration: {err:?}");
            let notification = gio::Notification::new("Clipulator configuration error");
            notification.set_body(Some(&err.to_string()));
            app.send_notification(None, &notification);
        }
    });

    application.connect_activate(build_ui);

    application.run();
}

fn build_ui(application: &gtk::Application) {
    let config = match load_config() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("Failed to load config: {err:?}");
            DEFAULT_CONFIG.clone()
        }
    };

    let history_state: Rc<RefCell<VecDeque<String>>> = Rc::new(RefCell::new(VecDeque::new()));
    let history_limit = config.history_size.max(1);

    let window = gtk::ApplicationWindow::new(application);
    window.set_title("Clipulator");
    window.set_default_size(480, 640);

    let header = gtk::HeaderBar::new();
    header.set_title(Some("Clipulator"));
    header.set_show_close_button(true);
    window.set_titlebar(Some(&header));

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.set_margin_top(12);
    outer.set_margin_bottom(12);
    outer.set_margin_start(12);
    outer.set_margin_end(12);

    let history_list = gtk::ListBox::new();
    history_list.set_selection_mode(gtk::SelectionMode::None);

    let history_frame = gtk::Frame::new(Some("Clipboard history"));
    history_frame.set_hexpand(true);
    history_frame.set_vexpand(true);
    history_frame.add(&history_list);

    let status_label = gtk::Label::new(Some("Ready"));
    status_label.set_line_wrap(true);
    status_label.set_xalign(0.0);

    let commands_label = gtk::Label::new(Some(&describe_commands(&config.commands)));
    commands_label.set_line_wrap(true);
    commands_label.set_xalign(0.0);

    outer.pack_start(&history_frame, true, true, 0);
    outer.pack_start(&commands_label, false, false, 0);
    outer.pack_start(&status_label, false, false, 0);

    window.add(&outer);
    window.show_all();

    let display = match Display::default() {
        Some(display) => display,
        None => {
            status_label.set_text("Unable to acquire a Wayland display");
            return;
        }
    };

    let clipboard = gtk::Clipboard::for_display(&display, &gdk::SELECTION_CLIPBOARD);

    let history_state_for_clipboard = history_state.clone();
    let history_list_for_clipboard = history_list.clone();
    let status_for_clipboard = status_label.clone();
    clipboard.connect_owner_change(move |clipboard, _| {
        let history_state = history_state_for_clipboard.clone();
        let history_list = history_list_for_clipboard.clone();
        let status_label = status_for_clipboard.clone();
        clipboard.request_text(move |_, text| match text {
            Some(text) => {
                if push_history(&history_state, text, history_limit) {
                    let history = history_state.borrow();
                    refresh_history(&history_list, &history);
                }
            }
            None => status_label.set_text("Clipboard is empty or not textual"),
        });
    });

    for command in config.commands.clone() {
        register_command(
            application,
            &command,
            clipboard.clone(),
            history_state.clone(),
            history_limit,
            history_list.clone(),
            status_label.clone(),
        );
    }
}

fn register_command(
    application: &gtk::Application,
    command: &CommandConfig,
    clipboard: gtk::Clipboard,
    history_state: Rc<RefCell<VecDeque<String>>>,
    history_limit: usize,
    history_list: gtk::ListBox,
    status_label: gtk::Label,
) {
    let action_name = format!("command-{}", command.id);
    let action = gio::SimpleAction::new(&action_name, None);

    let command_clone = command.clone();
    let status_clone = status_label.clone();
    let history_clone = history_state.clone();
    let history_list_clone = history_list.clone();
    let clipboard_clone = clipboard.clone();

    action.connect_activate(move |_, _| {
        status_clone.set_text(&format!("Running '{}'…", command_clone.name));
        let request_status = status_clone.clone();
        let request_history = history_clone.clone();
        let request_history_list = history_list_clone.clone();
        let request_command = command_clone.clone();
        let request_clipboard = clipboard_clone.clone();

        request_clipboard.request_text(move |clipboard_ref, text| match text {
            Some(text) => {
                let input = text.to_string();
                let command_for_thread = request_command.clone();
                let command_name = command_for_thread.name.clone();
                let history_state = request_history.clone();
                let history_list = request_history_list.clone();
                let status_for_receiver = request_status.clone();
                let clipboard_for_receiver = clipboard_ref.clone();
                let (sender, receiver) = MainContext::channel(PRIORITY_DEFAULT);

                receiver.attach(None, move |outcome: CommandOutcome| {
                    match outcome {
                        CommandOutcome::Completed { output } => {
                            clipboard_for_receiver.set_text(&output);
                            if push_history(&history_state, &output, history_limit) {
                                let history = history_state.borrow();
                                refresh_history(&history_list, &history);
                            }
                            status_for_receiver.set_text(&format!("'{}' applied", command_name));
                        }
                        CommandOutcome::Empty => {
                            status_for_receiver.set_text("Clipboard is empty or not textual");
                        }
                        CommandOutcome::Failed(err) => {
                            status_for_receiver
                                .set_text(&format!("'{}' failed: {err}", command_name));
                        }
                    }
                    Continue(false)
                });

                thread::spawn(move || {
                    let result = run_command(&command_for_thread, &input)
                        .map(|output| CommandOutcome::Completed { output })
                        .unwrap_or_else(|err| CommandOutcome::Failed(err.to_string()));
                    let _ = sender.send(result);
                });
            }
            None => request_status.set_text("Clipboard is empty or not textual"),
        });
    });

    application.add_action(&action);

    if let Some(accelerator) = command.accelerator.as_deref() {
        application.set_accels_for_action(&format!("app.{}", action_name), &[accelerator]);
    }
}

fn run_command(command: &CommandConfig, input: &str) -> Result<String> {
    if let Some(builtin) = &command.builtin {
        return Ok(match builtin {
            BuiltinCommand::Uppercase => input.to_uppercase(),
            BuiltinCommand::Lowercase => input.to_lowercase(),
            BuiltinCommand::Trim => input.trim().to_string(),
            BuiltinCommand::Titlecase => convert_title_case(input),
        });
    }

    let exec = command
        .exec
        .as_ref()
        .ok_or_else(|| anyhow!("no command configured"))?;

    if exec.is_empty() {
        return Err(anyhow!("command has no executable"));
    }

    let mut child = std::process::Command::new(&exec[0]);
    child.args(&exec[1..]);
    child.stdin(std::process::Stdio::piped());
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::piped());

    let mut child = child
        .spawn()
        .with_context(|| format!("failed to spawn '{}'", exec[0]))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .context("failed to write to command stdin")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to read command output")?;

    if !output.status.success() {
        return Err(anyhow!(
            "command exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let text = String::from_utf8(output.stdout).context("command output was not valid UTF-8")?;
    Ok(text)
}

fn convert_title_case(input: &str) -> String {
    input
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn ensure_default_config() -> Result<()> {
    let (path, exists) = config_path()?;
    if exists {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create configuration directory")?;
    }

    let default_json = serde_json::to_string_pretty(&*DEFAULT_CONFIG)
        .context("failed to serialize default configuration")?;
    fs::write(&path, default_json).context("failed to write default configuration")?;
    Ok(())
}

fn load_config() -> Result<AppConfig> {
    let (path, _) = config_path()?;
    let contents = fs::read_to_string(&path).context("failed to read configuration file")?;
    let config: AppConfig =
        serde_json::from_str(&contents).context("failed to parse configuration")?;
    Ok(config)
}

fn config_path() -> Result<(PathBuf, bool)> {
    let project_dirs = ProjectDirs::from("dev", "Clipulator", "Clipulator")
        .ok_or_else(|| anyhow!("failed to determine configuration directory"))?;
    let config_dir = project_dirs.config_dir().to_path_buf();
    let path = config_dir.join("config.json");
    Ok((path.clone(), path.exists()))
}

fn push_history(history_state: &Rc<RefCell<VecDeque<String>>>, text: &str, limit: usize) -> bool {
    if text.trim().is_empty() {
        return false;
    }

    let mut history = history_state.borrow_mut();

    if history
        .front()
        .map(|existing| existing == text)
        .unwrap_or(false)
    {
        return false;
    }

    history.push_front(text.to_string());
    while history.len() > limit {
        history.pop_back();
    }

    true
}

fn refresh_history(list: &gtk::ListBox, history: &VecDeque<String>) {
    for child in list.children() {
        list.remove(&child);
    }

    for (index, entry) in history.iter().enumerate() {
        let display_index = index + 1;
        let row = gtk::ListBoxRow::new();
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let title = gtk::Label::new(Some(&format!("#{display_index}")));
        title.set_xalign(0.0);
        let content = gtk::Label::new(Some(entry));
        content.set_xalign(0.0);
        content.set_line_wrap(true);
        inner.pack_start(&title, false, false, 0);
        inner.pack_start(&content, false, false, 0);
        row.add(&inner);
        list.add(&row);
    }

    list.show_all();
}

fn describe_commands(commands: &[CommandConfig]) -> String {
    if commands.is_empty() {
        return "No commands configured".into();
    }

    let mut lines = vec!["Commands".to_string()];

    for command in commands {
        let accelerator = command.accelerator.as_deref().unwrap_or("(no shortcut)");
        let description = command
            .description
            .as_deref()
            .or_else(|| command.builtin.as_ref().map(|builtin| builtin.describe()))
            .unwrap_or("Transforms clipboard using external command");
        lines.push(format!(
            "• {} — {} — {}",
            command.name, accelerator, description
        ));
    }

    lines.join("\n")
}

impl BuiltinCommand {
    fn describe(&self) -> &'static str {
        match self {
            BuiltinCommand::Uppercase => "Convert to uppercase",
            BuiltinCommand::Lowercase => "Convert to lowercase",
            BuiltinCommand::Trim => "Trim whitespace",
            BuiltinCommand::Titlecase => "Title Case",
        }
    }
}
