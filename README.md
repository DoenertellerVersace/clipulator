# Clipulator

Clipulator is a small Rust/GTK clipboard companion tailored for GNOME on Wayland. It keeps a searchable history of recent clipboard entries and lets you bind keyboard shortcuts to transformations implemented either in Rust or by piping the clipboard content through any external command.

This version targets the GTK 3 stack and drives the Wayland clipboard through the `gdk` crate. It demonstrates how to stay native on Wayland while still wiring custom accelerators to asynchronous work that reads, transforms and updates the clipboard in place.

## Features

- Text-only clipboard history stored in-memory (default 25 entries).
- Built-in transformations (uppercase, lowercase, trim, title case).
- Declarative configuration for custom shell commands that receive clipboard text on stdin and return the replacement on stdout.
- Application-scoped keyboard shortcuts for each command.
- Simple GTK 3 interface (via `gtk` + `gdk`) that displays history and command help.

> **Wayland note:** the application must be focused to access the clipboard. Global shortcuts and automatic "paste back" are intentionally constrained by Wayland for security reasons. Clipulator copies the transformed text to the clipboard so you can paste it with <kbd>Ctrl</kbd>+<kbd>V</kbd> in the target application.

## Requirements

- Rust 1.76+ (edition 2024).
- GTK 3.24+ development packages (`libgtk-3-dev` on Debian/Ubuntu, `gtk3` on Fedora/Arch, etc.).
- Rust `glib`/`gio`/`gdk` source packages when using system Rust crates (e.g. `librust-glib-dev`, `librust-glib-macros-dev`).
- A GNOME Wayland session.

Optional (only for external command recipes): any shell utilities you reference in the configuration file.

## Building and running

```bash
cargo run
```

On the first launch Clipulator writes a default configuration file to:

- Linux: `$XDG_CONFIG_HOME/Clipulator/config.json` (typically `~/.config/Clipulator/config.json`).

You can edit the file while the application is running; restart the app to apply changes.

## Configuration

Example configuration (`~/.config/Clipulator/config.json`):

```json
{
  "history_size": 30,
  "commands": [
    {
      "id": "uppercase",
      "name": "Uppercase",
      "accelerator": "<Primary><Shift>U",
      "builtin": "uppercase"
    },
    {
      "id": "slugify",
      "name": "Slugify",
      "description": "Convert spaces to dashes using sed",
      "accelerator": "<Primary><Shift>S",
      "exec": ["sh", "-c", "tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9\-\s]/-/g' | tr -s ' ' '-'" ]
    }
  ]
}
```

### Command fields

| Field | Description |
| --- | --- |
| `id` | Stable identifier for the action. Must be unique. |
| `name` | Human friendly name displayed in the UI. |
| `accelerator` | GTK accelerator string (e.g. `<Primary><Shift>U`). |
| `builtin` | One of `"uppercase"`, `"lowercase"`, `"trim"`, or `"titlecase"`. |
| `exec` | Array describing the external program to run. The clipboard text is written to its stdin; stdout replaces the clipboard when the process exits successfully. |
| `description` | Optional description shown in the UI. |
| `paste` | Reserved for future work (currently behaves the same as omitting it). |

Either `builtin` or `exec` must be provided.

### Adding shell transformations

External commands run without a shell unless you explicitly invoke one. For example, to wrap the clipboard in backticks you can use:

```json
{
  "id": "backticks",
  "name": "Markdown inline code",
  "accelerator": "<Primary><Shift>M",
  "exec": ["sh", "-c", "printf '\`%s\`' "],
  "description": "Wrap text in backticks"
}
```

The command must write valid UTF-8 to stdout. Non-zero exit codes leave the clipboard unchanged and display the error message in the status area.

## Keyboard shortcuts

Accelerators are active while the Clipulator window is focused. To use a shortcut:

1. Copy or select text in the source application.
2. Focus Clipulator (<kbd>Super</kbd> key and type "Clipulator" on GNOME).
3. Trigger the desired shortcut.
4. Switch back to the target application and paste.

## Development

- Format: `cargo fmt`
- Lint: `cargo clippy`
- Check: `cargo check`

### Dev container (VS Code / Dev Containers extension)

You can avoid installing the native GTK development headers on your host by using the provided development container definition. This setup builds on top of the official Rust devcontainer image and installs the libraries required by the `gtk`, `gdk`, and `glib` crates.

1. Install the [Dev Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers) extension for VS Code.
2. Run **Dev Containers: Reopen in Container** from the command palette while this repository is open.
3. Wait for the container to build; it will pre-fetch the crate dependencies automatically.
4. Use the integrated terminal to run `cargo check`, `cargo clippy`, and `cargo run` inside the container.

### Notes

The code favours clear, blocking-free clipboard access by dispatching transformations to worker threads and posting the results back through the GTK main loop. This pattern keeps the UI responsive while still leveraging synchronous Rust APIs and external processes.

## Roadmap / ideas

- Persist history across restarts.
- Integrate with system notifications when commands succeed/fail.
- Explore xdg-desktop-portal Remote Desktop APIs for secure "paste back" automation.
