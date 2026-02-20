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
| `g` / `Home` | Jump to top |
| `G` / `End` | Jump to bottom |
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

**Search**

| Key | Action |
|-----|--------|
| `/` | Open search bar |
| `/query` | Fuzzy match (default) |
| `/r:pattern` | Regex |
| `/s:text` | Substring |

**Other**

| Key | Action |
|-----|--------|
| `?` | Toggle help popup |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## Colour Scheme

- **Green** `[+]` — Added entries
- **Red** `[-]` — Removed entries
- **Yellow** `[~]` — Modified entries

## Requirements

Requires a terminal with colour support (most modern terminals).
