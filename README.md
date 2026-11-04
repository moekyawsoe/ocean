<div align="center">

<img src="./assets/icons/ocean.svg" alt="Ocean Terminal Logo" width="128" height="128" />

# Ocean Terminal (`ocean`)

**Clean, minimal, ultra-lightweight GPU-accelerated terminal emulator written in Rust with full Myanmar / Burmese (`မြန်မာဘာသာ`) OpenType shaping support and Ghostty-style configuration.**

[![Release](https://img.shields.io/github/v/release/moekyawsoe/ocean?include_prereleases&style=flat-square&color=0284c7)](https://github.com/moekyawsoe/ocean/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-0ea5e9.svg?style=flat-square)](./LICENSE)
[![Platform: Linux](https://img.shields.io/badge/Platform-Linux%20(Wayland%20%2F%20X11)-38bdf8.svg?style=flat-square)](https://github.com/moekyawsoe/ocean)

[Landing Page Website](./website/index.html) • [Download (.deb / .rpm)](#download--installation) • [Highlights](#highlights) • [CLI Actions](#ghostty-style-cli-actions) • [Config](#configuration-configoceanconfig) • [Shortcuts](#default-shortcuts)

</div>

---

## Screenshots

### 1. Ocean Dark Theme & Native Myanmar (`mym2`) OpenType Shaping
![Ocean Terminal - Myanmar Shaping & Ocean Dark Theme](./assets/screenshots/ocean-myanmar-dark.png)

### 2. Side-by-Side Split Panes, Tabs & Vim-Navigable Theme Context Menu
![Ocean Terminal - Split Panes & Context Menu](./assets/screenshots/ocean-splits-menu.png)

### 3. Ocean Light Theme (High Contrast)
![Ocean Terminal - Ocean Light Theme](./assets/screenshots/ocean-light.png)

---

## Download & Installation

Pre-built `.deb`, `.rpm`, and portable `.tar.gz` packages are built automatically by GitHub Actions on every tag push (e.g. `v1.0.0`).

### Debian / Ubuntu / Linux Mint / Pop!_OS (`.deb`)

```bash
# Download and install the latest .deb package
curl -LO https://github.com/moekyawsoe/ocean/releases/latest/download/ocean_amd64.deb
sudo apt install ./ocean_amd64.deb
```

### Fedora / RHEL / Rocky Linux / openSUSE (`.rpm`)

```bash
# Install directly via DNF
sudo dnf install https://github.com/moekyawsoe/ocean/releases/latest/download/ocean.x86_64.rpm
```

### Build from Source (Cargo)

```bash
git clone https://github.com/moekyawsoe/ocean.git
cd ocean
cargo build --release
./target/release/ocean
```

---

## Highlights

- **Ultra-Lightweight & Fast**: Single-draw-call OpenGL 3.3 / GLES 3.0 instanced GPU renderer (`glutin` + `glow`), shelf-packed `swash` glyph & color emoji atlas, O(1) `FxHashMap` shaped-run memoization, coalesced epoll PTY reader, and **0% idle CPU** (`ControlFlow::Wait`).
- **Full Myanmar / Burmese Support (`U+1000..U+109F`, `U+AA60..U+AA7F`, `U+A9E0..U+A9FF`)**:
  - **HarfBuzz (`rustybuzz`) Run-Based OpenType `mym2`/`mymr` Shaping**: Properly shapes Kinzi (`င်္ဂ`), pre-base vowel `ေ`, medial `ြ ျ ွ ှ` (narrow & wide consonant variants), stacked subjoined consonants (`ဗုဒ္ဓ`, `ဣန္ဒြေ`, `မန္တလေး`, `တက္ကသိုလ်`), tall vs round `ါ`/`ာ` (`ပါ`), `ဦး`, `၎င်း`, and Myanmar digits (`၀-၉`) with **zero dotted circles**.
  - **UTN #11 Combining Order Normalization**: Automatically normalizes swapped typing sequences (e.g. `ကုိ` → `ကို`, `ကေြ` → `ကြေ`) so minor typing-order variations never break syllables.
  - **Vertical Metrics & 2-Layer Compositing**: Upper (`ိ ီ ံ င်္`) and lower (`ု ူ ြ ွ ှ ္`) marks never get clipped by adjacent rows, while procedural box-drawing (`╭─┬─╮`) keeps TUI borders razor-sharp.
  - **Full Burmese Input Method (IME) & XKB Support**: Native Wayland (`zwp_text_input_v3`) & X11 (`XIM`) IME support with live shaped Burmese preedit overlay at the cursor, plus XKB `mm` layout & Wayland/X11 UTF-8 clipboard support.
  - **Optional Legacy Zawgyi-One Display Mode**: Set `myanmar-zawgyi = auto` (or press `Ctrl+Shift+M`) to read legacy Zawgyi logs/files seamlessly in Unicode fonts.
- **Keyboard-First Developer Workflow & Neovim-Style Bottom Bar**:
  - Clean bottom mode & shortcut status bar (`OCEAN`, `TUI`, `SCROLL`, `MENU`, `THEME`, `HELP`) showing live context-aware keybindings, active split indicator, and theme pill (toggleable via `Ctrl+Shift+B` or `window-show-status-bar = false`).
  - Open the Context Menu anytime from the keyboard with `Ctrl+Shift+P` (or `Menu` / `Shift+F10`) anchored right at the terminal cursor, and navigate with Vim keys (`j`/`k`, `t` for themes, `1`–`9` quick-picks, single-key mnemonics, `h`/`l`, `Esc`/`q`).
  - Full keyboard split management: focus (`Alt+Shift+H/J/K/L` or `Alt+Shift+Arrows`), resize (`Ctrl+Alt+H/J/K/L` or `Ctrl+Alt+Arrows`), and equalize (`Alt+Shift+=`).
- **Ghostty-Compatible Config (`~/.config/ocean/config`)**:
  - Clean `key = value` syntax — supports `font-family`, `font-size`, `theme`, `background`, `foreground`, `background-opacity`, `palette`, `cursor-style`, `window-padding-x/y`, `adjust-cell-height/width`, `keybind`, `config-file`, and **live hot-reload** on save or `Ctrl+Shift+,` / `Ctrl+Shift+R`.

---

## Ghostty-Style CLI Actions

```bash
# Generate starter Ghostty-style config at ~/.config/ocean/config
ocean +init-config

# List discovered Monospace & Myanmar/Burmese system fonts
ocean +list-fonts

# List built-in color themes (5 Dark + 5 Light)
ocean +list-themes

# Verify Myanmar/Burmese OpenType shaping on your system
ocean +myanmar-check

# Show active resolved configuration
ocean +show-config

# Validate config file syntax
ocean +validate-config

# Launch with a specific theme or command
ocean --theme=catppuccin-mocha
ocean -e htop
```

---

## Configuration (`~/.config/ocean/config`)

See [`config.example`](./config.example) for all available options:

```ini
# Typography
font-family = Cascadia Code NF
font-family-myanmar = Padauk
font-size = 13.5
myanmar-font-scale = 0.96
myanmar-zawgyi = off

# Theme & Appearance (10 curated themes: ocean-dark, tokyo-night, catppuccin-mocha, nord, ocean-light, etc.)
theme = ocean-dark
background-opacity = 0.96
cursor-style = block
window-titlebar = false
window-show-status-bar = true
window-padding-x = 10,10
window-padding-y = 8,8

# Keybindings
keybind = ctrl+shift+p=toggle_context_menu
keybind = ctrl+shift+b=toggle_status_bar
keybind = ctrl+shift+c=copy_to_clipboard
keybind = ctrl+shift+v=paste_from_clipboard
keybind = ctrl+shift+t=new_tab
keybind = ctrl+shift+d=new_split:right
keybind = ctrl+shift+e=new_split:down
keybind = alt+shift+h=goto_split:left
keybind = alt+shift+l=goto_split:right
keybind = ctrl+alt+h=resize_split:left,40
keybind = ctrl+alt+l=resize_split:right,40
keybind = alt+shift+equal=equalize_splits
keybind = ctrl+shift+w=close_surface
keybind = ctrl+shift+comma=reload_config
keybind = ctrl+shift+m=toggle_myanmar_zawgyi
```

---

## Default Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+Shift+P` / `Menu` / `Shift+F10` | Open / close Context Menu at terminal cursor (navigate with `j`/`k`, `t` themes, `1`–`9`, mnemonics) |
| `Ctrl+Shift+B` | Show / hide bottom Neovim-style shortcut bar |
| `Ctrl+Shift+C` / `Ctrl+Insert` | Copy selection to clipboard |
| `Ctrl+Shift+V` / `Shift+Insert` | Paste from clipboard |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Increase / decrease / reset font size |
| `Ctrl+Shift+T` / `Ctrl+Shift+W` | New tab / close split or tab (tab bar auto-hides when 1 tab) |
| `Ctrl+Shift+D` / `Ctrl+Shift+E` | Split pane right / split pane down |
| `Alt+Shift+H/J/K/L` or `Alt+Shift+Arrows` | Move focus between split panes |
| `Ctrl+Alt+H/J/K/L` or `Ctrl+Alt+Arrows` | Resize active split pane |
| `Alt+Shift+=` | Equalize split pane sizes |
| `Right Click` | Context menu at pointer (also clickable via bottom bar pills) |
| `Ctrl+Shift+?` | Toggle in-app Help & Documentation dialog (`1`–`5` / `h`/`l` sections, `j`/`k`/`g`/`G` scroll) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+Shift+N` | Open new window |
| `Ctrl+Shift+,` or `Ctrl+Shift+R` | Live reload configuration |
| `Ctrl+Shift+M` | Toggle legacy Zawgyi-One auto-conversion |
| `Shift+PageUp` / `Shift+PageDown` | Scroll history page up / down |
| `Ctrl+Left Click` | Open URL under cursor |
| `F11` | Toggle fullscreen |
