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

### Summary Mode

For non-interactive output, use the `--summary` flag to print a structured text report to stdout and exit:

```sh
lp_diff base.lp modified.lp --summary
```

Output format:

```
LP Diff: base.lp vs modified.lp

Variables:    +3   -1   ~2   (42 unchanged)
Constraints:  +0   -5   ~12  (300 unchanged)
Objectives:   +0   -0   ~1   (0 unchanged)

Total: 24 changes
```

## Layout

The interface is a three-panel layout:

| Panel | Description |
|-------|-------------|
| Section Selector | Left sidebar — choose between Summary, Variables, Constraints, and Objectives |
| Name List | Left sidebar — filterable list of changed entries for the selected section |
| Detail | Right panel — full diff detail for the selected entry |

The status bar at the bottom shows total changes, per-section diff statistics (`+N -N ~N`), the active filter, and scroll position.

Press `?` at any time to open the key bindings pop up.

### Sections

| # | Section | Description |
|---|---------|-------------|
| 1 | Summary | Overview of change counts, problem dimensions, and structural analysis |
| 2 | Variables | Variable type changes |
| 3 | Constraints | Constraint changes with coefficient-level detail (side-by-side view for modified) |
| 4 | Objectives | Objective function changes |

### Side-by-Side Constraint View

Modified standard constraints are displayed in a two-column layout showing old and new coefficients side by side. Added coefficients are highlighted in green, removed in red, and modified in yellow. Unchanged coefficients appear in grey.

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
| `Ctrl+o` | Jump back (jumplist) |
| `Ctrl+i` | Jump forward (jumplist) |
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
| `Tab` | Complete query with selected result's name |
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

**Solver**

| Key | Action |
|-----|--------|
| `S` | Solve an LP file with HiGHS |
| `1` / `2` | Select file 1 or file 2 (in picker) |
| `j` / `k` | Scroll results (in results view) |
| `y` | Yank solve results to clipboard |
| `Esc` | Close solver overlay |

**Other**

| Key | Action |
|-----|--------|
| `?` | Toggle help pop-up |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## HiGHS Solver

Press `S` to solve either LP file on demand using the [HiGHS](https://highs.dev) solver. Pick file 1 or 2, and the solver runs in a background thread. Results show the optimisation status, objective value, solve time, and a scrollable variable table.

## Jumplist

Navigation positions are recorded automatically when you change sections, apply filters, or jump to a search result. Use `Ctrl+o` to go back and `Ctrl+i` to go forward through your navigation history (up to 100 positions).

## Colour Scheme

- **Green** `[+]` — Added entries
- **Red** `[-]` — Removed entries
- **Yellow** `[~]` — Modified entries

## Requirements

Requires a terminal with colour support (most modern terminals).
