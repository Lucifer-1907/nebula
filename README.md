<p align="center">
  <img src="https://img.shields.io/badge/rust-1.70%2B-orange?logo=rust&logoColor=white" alt="Rust 1.70+">
  <img src="https://img.shields.io/badge/platform-Linux-blue?logo=linux&logoColor=white" alt="Linux">
  <img src="https://img.shields.io/badge/TUI-ratatui-purple" alt="ratatui">
  <img src="https://img.shields.io/badge/license-MIT-green" alt="MIT License">
</p>

<h1 align="center">🌌 Nebula</h1>

<p align="center">
  <strong>A blazing-fast, GUI-quality terminal file manager written in Rust</strong>
</p>

<p align="center">
  <em>Vim-inspired keybindings · Miller column layout · Async I/O · Real-time filesystem watching · Image previews</em>
</p>

---

## ✨ Features

### 🚀 Core

- **Miller Column Layout** — Three-pane view (parent · current · preview) for spatial context while navigating
- **Vim-Modal Editing** — Four distinct modes: **Normal**, **Visual**, **Command**, and **Input**
- **Async Everything** — All filesystem I/O runs on the Tokio runtime; the UI thread never blocks
- **Real-Time Filesystem Watching** — Powered by `notify` (inotify/kqueue/FSEvents) with 300ms debouncing for live reload on external changes

### 🎨 Design

- **Catppuccin Mocha Theme** — Curated dark palette with precise RGB colors for true-color terminals
- **Nerd Font Icons** — 100+ file-type/directory icons with contextual coloring (Rust, Go, Python, JS/TS, Docker, and more)
- **Animated UI** — Loading spinners, auto-dismissing status messages, and smooth cursor wrapping

### 📂 File Management

- **CRUD Operations** — Create files (`a`), create directories (`A`), rename (`r`), and delete (`d`) with confirmation dialogs
- **Multi-Selection** — Visual mode range selection, `Space` toggling, and bulk delete
- **Smart Sorting** — Cycle through Name, Size, Modified, and Extension sort modes (directories always first)
- **Hidden File Toggle** — Show/hide dotfiles with a single keystroke (`.`)
- **Open with OS Default** — Press `Enter` on a file to launch it with `xdg-open`

### 🖼️ Preview

- **Directory Preview** — Instantly preview subdirectory contents in the right pane
- **Image Preview** — Render image thumbnails directly in the terminal (PNG, JPEG, WebP, GIF, BMP, ICO, TIFF) via `ratatui-image`
- **Cancellable Previews** — Rapid scrolling cancels stale preview tasks to keep the UI responsive
- **Permission Denied / Broken Symlink Handling** — Graceful error states with descriptive icons

### 🧠 Quality of Life

- **Cursor Memory** — Remembers your cursor position per-directory so you return exactly where you left off
- **Breadcrumb Trail** — Styled path segments at the top for spatial orientation
- **Rich Status Bar** — Mode indicator, file permissions (`rwxrwxrwx`), owner:group, size, modified date, selection count, position, and sort mode
- **Command Mode** — Ex-style `:` commands (`:q`, `:mkdir`, `:touch`, `:sort`, `:hidden`, `:help`)
- **Graceful Terminal Restoration** — RAII guard + panic hook ensures the terminal is always restored, even on crashes

---

## 📦 Installation

### Prerequisites

- **Rust** 1.70+ (install via [rustup](https://rustup.rs/))
- **A Nerd Font** — Required for file-type icons ([Nerd Fonts](https://www.nerdfonts.com/))
- **True-color terminal** — For the full Catppuccin color experience (most modern terminals support this)
- **Linux** — Uses `xdg-open` for file launching and Unix-specific metadata APIs

### Build from Source

```bash
git clone https://github.com/Aanand010907/nebula.git
cd nebula
cargo build --release
```

The optimized binary will be at `./target/release/nebula`.

### Run

```bash
# Launch in the current directory
./target/release/nebula

# Launch in a specific directory
./target/release/nebula /path/to/directory
```

### Install to PATH

```bash
cargo install --path .
```

---

## ⌨️ Keybindings

### Normal Mode

| Key | Action |
|-----|--------|
| `j` / `↓` | Move cursor down |
| `k` / `↑` | Move cursor up |
| `h` / `←` | Go to parent directory |
| `l` / `→` / `Enter` | Enter directory / Open file |
| `g` | Jump to first item |
| `G` | Jump to last item |
| `Ctrl+d` | Page down |
| `Ctrl+u` | Page up |
| `a` | Create new file |
| `A` | Create new directory |
| `r` | Rename current item |
| `d` / `Delete` | Delete item(s) |
| `Space` | Toggle selection |
| `s` | Cycle sort mode |
| `.` | Toggle hidden files |
| `v` | Enter Visual mode |
| `:` | Enter Command mode |
| `?` / `Ctrl+k` | Show help menu |
| `q` / `Ctrl+c` | Quit |

### Visual Mode

| Key | Action |
|-----|--------|
| `j` / `k` | Move + extend selection |
| `Space` | Toggle current item |
| `a` | Select all |
| `d` | Delete selected |
| `Esc` / `v` | Exit Visual mode |

### Command Mode

| Command | Action |
|---------|--------|
| `:q` / `:quit` | Quit |
| `:mkdir <name>` | Create directory |
| `:touch <name>` | Create file |
| `:sort` | Cycle sort mode |
| `:hidden` | Toggle hidden files |
| `:help` | Show keybinding help |

---

## 🏗️ Architecture

Nebula uses a **unidirectional data flow** architecture, where every state mutation flows through a single `Action` enum — making the data flow predictable and easy to reason about.

```
┌─────────────┐      ┌─────────┐      ┌───────────┐      ┌──────────┐
│ EventHandler│────▶│ Input   │────▶│  Action   │────▶│   App    │
│ (crossterm) │      │ Handler │      │ (dispatch)│      │ (state)  │
└─────────────┘      └─────────┘      └───────────┘      └────┬─────┘
                                                              │
                         ┌────────────────────────────────────┘
                         ▼
                    ┌──────────┐     ┌────────────────┐
                    │    UI    │     │  Async Workers │
                    │ (render) │     │ (tokio::spawn) │
                    └──────────┘     └────────────────┘
```

### Module Structure

```
src/
├── main.rs              # Entry point, terminal setup, event loop
├── app.rs               # Central state + dispatch logic
├── action.rs            # Action enum (all possible state mutations)
├── event.rs             # Event handler (crossterm → AppEvent)
├── input/
│   ├── handler.rs       # Keymap: KeyEvent → Action
│   └── keymap.rs        # Keymap utilities
├── state/
│   ├── mode.rs          # Modal editing modes (Normal/Visual/Command/Input)
│   ├── selection.rs     # Multi-selection state management
│   └── tab.rs           # Tab state, cursor history, entry management
├── theme/
│   ├── palette.rs       # Catppuccin Mocha color palette + pre-built styles
│   └── icons.rs         # 100+ Nerd Font icon mappings by file type
├── ui/
│   ├── mod.rs           # Master render function
│   ├── layout.rs        # Miller column layout calculations
│   ├── column.rs        # Parent + current column rendering
│   ├── preview.rs       # Preview pane (dirs, images, errors)
│   ├── breadcrumb.rs    # Path breadcrumb trail
│   ├── statusbar.rs     # Status bar with metadata
│   ├── command_line.rs  # Command mode input line
│   ├── dialog.rs        # Floating input/confirm dialogs
│   └── help.rs          # Floating keybinding help menu
└── vfs/
    ├── entry.rs         # FileEntry struct with rich metadata
    ├── scanner.rs       # Async directory scanning + preview loading
    ├── ops.rs           # File operations (create, rename, delete, open)
    ├── sort.rs          # Sort modes (Name, Size, Modified, Extension)
    └── watcher.rs       # inotify/kqueue filesystem watcher with debouncing
```

---

## 🔧 Dependencies

| Crate | Purpose |
|-------|---------|
| [`ratatui`](https://crates.io/crates/ratatui) | Terminal UI rendering framework |
| [`crossterm`](https://crates.io/crates/crossterm) | Cross-platform terminal manipulation |
| [`tokio`](https://crates.io/crates/tokio) | Async runtime for non-blocking I/O |
| [`notify`](https://crates.io/crates/notify) | Filesystem event watching (inotify/kqueue) |
| [`ratatui-image`](https://crates.io/crates/ratatui-image) | In-terminal image rendering |
| [`image`](https://crates.io/crates/image) | Image decoding (JPEG, PNG, WebP, etc.) |
| [`nix`](https://crates.io/crates/nix) | Unix filesystem metadata & permissions |
| [`humansize`](https://crates.io/crates/humansize) | Human-readable file sizes |
| [`chrono`](https://crates.io/crates/chrono) | Date/time formatting |
| [`anyhow`](https://crates.io/crates/anyhow) / [`thiserror`](https://crates.io/crates/thiserror) | Error handling |
| [`unicode-width`](https://crates.io/crates/unicode-width) | Proper Unicode character alignment |

---

## 🎯 Release Profile

Nebula ships with an aggressive release profile for maximum performance:

```toml
[profile.release]
opt-level = 3       # Maximum optimization
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit for best optimization
strip = true        # Strip debug symbols for smaller binary
```

---

## 🗺️ Roadmap

- [ ] Text file content preview
- [ ] Fuzzy search / filter
- [ ] Copy / Cut / Paste operations
- [ ] Multi-tab support
- [ ] Bookmark / favorite directories
- [ ] Configurable keybindings (TOML/YAML)
- [ ] Customizable themes
- [ ] Trash bin support (instead of permanent delete)
- [ ] macOS `open` / Windows `start` support

---

## 🤝 Contributing

Contributions are welcome! Feel free to open issues and pull requests.

1. Fork the repository
2. Create your feature branch (`git checkout -b feat/amazing-feature`)
3. Commit your changes (`git commit -m 'feat: add amazing feature'`)
4. Push to the branch (`git push origin feat/amazing-feature`)
5. Open a Pull Request

---

## 📝 License

This project is open source. See the repository for license details.

---

<p align="center">
  Built with 🦀 Rust and ❤️ by <a href="https://github.com/Aanand010907">Aanand010907</a>
</p>
