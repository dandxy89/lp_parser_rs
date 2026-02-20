# lp_diff — Interactive LP File Diff Viewer

A terminal-based interactive diff viewer for Linear Programming (LP) files, built with [ratatui](https://ratatui.rs).

## Installation

```sh
cargo install --path tui
```

Or run directly from the workspace:

```sh
cargo run -p lp_parser_tui -- file1.lp file2.lp
```

## Usage

```sh
lp_diff base.lp modified.lp
```

The viewer parses both LP files, computes a rich diff report, and launches an interactive TUI.

## Layout

The interface is a three-panel layout:

| Panel | Description |
|-------|-------------|
| Section Selector | Left sidebar — choose between Summary, Variables, Constraints, and Objectives |
| Name List | Left sidebar — filterable list of changed entries for the selected section |
| Detail | Right panel — full diff detail for the selected entry |

Press `?` at any time to open the key bindings pop up.

### Sections

| # | Section | Description |
|---|---------|-------------|
| 1 | Summary | Overview of change counts |
| 2 | Variables | Variable type changes |
| 3 | Constraints | Constraint changes with coefficient-level detail |
| 4 | Objectives | Objective function changes |

### Key Bindings

**Navigation**

| Key | Action |
|-----|--------|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `n` | Next match (or down when no search) |
| `N` | Previous match (or up when no search) |
| `g` / `Home` | Jump to top |
| `G` / `End` | Jump to bottom |
| `Ctrl+d` | Half page down |
| `Ctrl+u` | Half page up |
| `Ctrl+f` | Full page down |
| `Ctrl+b` | Full page up |
| `Tab` | Next panel |
| `Shift+Tab` | Previous panel |
| `Enter` | Go to detail panel |
| `h` / `l` | Move to sidebar / detail |
| `1`–`4` | Jump to section by number |
| `Esc` | Back / clear search |

**Filters**

| Key | Action |
|-----|--------|
| `a` | All changes |
| `+` | Added only |
| `-` | Removed only |
| `m` | Modified only |

**Search (Telescope-style pop-up)**

| Key | Action |
|-----|--------|
| `/` | Open search pop-up (searches across all sections) |
| `j` / `↓` | Next result (in pop-up) |
| `k` / `↑` | Previous result (in pop-up) |
| `Enter` | Jump to selected entry |
| `Esc` | Cancel search |
| `n` / `N` | Next / previous match (main view, when search was committed) |

Search mode prefixes (type in the pop-up input):

| Prefix | Mode |
|--------|------|
| *(none)* | Fuzzy match (default, ranked by score) |
| `r:` | Regex (case-insensitive) |
| `s:` | Substring (case-insensitive) |

**Clipboard**

| Key | Action |
|-----|--------|
| `y` | Yank selected entry name to clipboard |
| `Y` | Yank full detail panel content to clipboard |

**Other**

| Key | Action |
|-----|--------|
| `?` | Toggle help pop-up |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## Colour Scheme

- **Green** `[+]` — Added entries
- **Red** `[-]` — Removed entries
- **Yellow** `[~]` — Modified entries

## Requirements

Requires a terminal with colour support (most modern terminals).
