# lp_diff — LP/MPS Model Explorer and Diff Viewer

A terminal-based interactive explorer and diff viewer for Linear Programming files (LP and MPS formats), built with [ratatui](https://ratatui.rs). Pass one file to explore a single model, or two files to diff them.

![lp_diff demo](assets/demo.gif)

The demo runs two scenes. First it diffs `fit2d.mps` against `fit1d.lp` (mixed formats), touring the coefficient-level constraint diff, the raw text view, live tolerance cycling, fuzzy search, Numerics, and a parallel HiGHS solve with solution diff. Then it inspects `boeing2.lp` on its own to apply the presolve rewrites (`P`) and open the diagnostics pane (`D`).

Regenerate it with `vhs tui/scripts/demo.tape && gifsicle -O3 --batch tui/assets/demo.gif` (from the repo root).

## Installation

```sh
cargo install --path tui
```

Or run directly from the workspace:

```sh
cargo run -p lp_parser_tui -- file1.lp file2.mps
```

## Usage

Inspect a single model:

```sh
lp_diff model.lp
```

Diff two files:

```sh
lp_diff base.lp modified.lp
```

Both LP (`.lp`) and MPS (`.mps`) formats are supported. You can even mix formats — compare an LP file against an MPS file:

```sh
lp_diff model.lp model.mps
```

With one file the viewer parses it and launches a single-model explorer (inspect mode); with two files it computes a rich diff report and launches the diff viewer. Format is detected automatically by file extension.

### Inspect Mode (single file)

Given one file, the same five sections describe the single model rather than a comparison:

- **Summary** — file path, problem name, sense, per-section counts, and the structural analysis (dimensions, variable/constraint types, coefficient scaling, issues).
- **Variables / Constraints / Objectives** — every entry in the model, listed plainly (no diff badges). The detail panel shows the full entry: coefficients with names, operator and RHS, bounds and variable type, or SOS weights.
- **Numerics** — the per-file numerical conditioning view.

Search (`/`), the command palette (`Ctrl+p`), sort by name, the HiGHS solver (`S`, solving the single model directly), CSV export (`w`, writing `objectives.csv`, `constraints.csv`, `variables.csv` via the core library), and `--watch` all work. Diff-only actions — kind filters (`a`/`+`/`-`/`m`/`=`), ignore-order (`o`), tolerance cycling (`t`/`T`), delta sorts, and the raw side-by-side view (`r`) — are hidden from the help/palette and no-op with a brief status-bar hint if pressed.

### Summary Mode

For non-interactive output, use the `--summary` flag to print a structured text report to stdout and exit. In diff mode it prints the change summary; in inspect mode it prints the model counts and top analysis issues:

```sh
lp_diff base.lp modified.lp --summary   # diff summary
lp_diff model.lp --summary              # single-model summary
```

Output format:

```text
LP Diff: base.lp vs modified.lp

Variables:    +3   -1   ~2   (42 unchanged)
Constraints:  +0   -5   ~12  (300 unchanged)
Objectives:   +0   -0   ~1   (0 unchanged)

Total: 24 changes
```

## Layout

The interface is a three-panel layout:

| Panel            | Description                                                                             |
| ---------------- | --------------------------------------------------------------------------------------- |
| Section Selector | Left sidebar — choose between Summary, Variables, Constraints, Objectives, and Numerics |
| Name List        | Left sidebar — filterable list of changed entries for the selected section              |
| Detail           | Right panel — full diff detail for the selected entry                                   |

The status bar at the bottom shows total changes, per-section diff statistics (`+N -N ~N`), the active filter, and scroll position.

Press `?` at any time to open the key bindings pop up.

### Sections

| #   | Section     | Description                                                                       |
| --- | ----------- | --------------------------------------------------------------------------------- |
| 1   | Summary     | Overview of change counts, problem dimensions, and structural analysis            |
| 2   | Variables   | Variable type changes                                                             |
| 3   | Constraints | Constraint changes with coefficient-level detail (side-by-side view for modified) |
| 4   | Objectives  | Objective function changes                                                        |
| 5   | Numerics    | Per-file numerical conditioning view (coefficient scaling, ranges, issues)        |

### Side-by-Side Constraint View

Modified standard constraints are displayed in a two-column layout showing old and new coefficients side by side. Added coefficients are highlighted in green, removed in red, and modified in yellow. Unchanged coefficients appear in grey.

### Raw Text View

Press `r` in the detail panel to toggle between the parsed diff view and a side-by-side raw text view showing the actual LP file lines for the selected constraint or objective. The left column shows the file 1 text and the right column shows the file 2 text.

### CSV Export

In diff mode, press `w` to export the full diff report as a CSV file (`lp_diff_report_<timestamp>.csv`) in the current directory. The CSV includes all sections with columns for section, name, change type, and detail. In inspect mode, `w` exports the model itself as `objectives.csv`, `constraints.csv`, and `variables.csv` (via the core library's `to_csv`).

### Key Bindings

**Navigation**

| Key          | Action                    |
| ------------ | ------------------------- |
| `j` / `↓`    | Move down                 |
| `k` / `↑`    | Move up                   |
| `n`          | Move down                 |
| `N`          | Move up                   |
| `g` / `Home` | Jump to top               |
| `G` / `End`  | Jump to bottom            |
| `Ctrl+d`     | Half page down            |
| `Ctrl+u`     | Half page up              |
| `Ctrl+f`     | Full page down            |
| `Ctrl+b`     | Full page up              |
| `Ctrl+o`     | Jump back (jumplist)      |
| `Ctrl+i`     | Jump forward (jumplist)   |
| `Tab`        | Next panel                |
| `Shift+Tab`  | Previous panel            |
| `Enter`      | Go to detail panel        |
| `h` / `l`    | Move to sidebar / detail  |
| `1`–`5`      | Jump to section by number |
| `Esc`        | Back / clear search       |

**Filters**

| Key | Action                                   |
| --- | ---------------------------------------- |
| `a` | All changes                              |
| `+` | Added only                               |
| `-` | Removed only                             |
| `m` | Modified only                            |
| `=` | Renamed only                             |
| `o` | Toggle ignore-coefficient-order matching |

**Search (Telescope-style pop-up)**

| Key       | Action                                                       |
| --------- | ------------------------------------------------------------ |
| `/`       | Open search pop-up (searches across all sections)            |
| `j` / `↓` | Next result (in pop-up)                                      |
| `k` / `↑` | Previous result (in pop-up)                                  |
| `Tab`     | Complete query with selected result's name                   |
| `Enter`   | Jump to selected entry                                       |
| `Esc`     | Cancel search                                                |
| `n` / `N` | Next / previous match (main view, when search was committed) |

Search mode prefixes (type in the pop-up input):

| Prefix   | Mode                                   |
| -------- | -------------------------------------- |
| *(none)* | Fuzzy match (default, ranked by score) |
| `r:`     | Regex (case-insensitive)               |
| `s:`     | Substring (case-insensitive)           |

**Clipboard**

| Key  | Action                                          |
| ---- | ----------------------------------------------- |
| `yy` | Yank selected entry name to clipboard           |
| `yo` | Yank old (file 1) version of entry to clipboard |
| `yn` | Yank new (file 2) version of entry to clipboard |
| `Y`  | Yank full detail panel content to clipboard     |

**Solver**

| Key                 | Action                                                                         |
| ------------------- | ------------------------------------------------------------------------------ |
| `S`                 | Solve problem with HiGHS                                                       |
| `1` / `2`           | Select file 1 or file 2 (in picker)                                            |
| `3`                 | Solve both and diff (in picker)                                                |
| `1`–`5`             | Switch tab — Summary / Variables / Constraints / Log / Duals (in results view) |
| `Tab` / `Shift+Tab` | Cycle result tabs forward / backward                                           |
| `j` / `k`           | Scroll results (in results view)                                               |
| `d`                 | Toggle diff-only filter (both mode)                                            |
| `t` / `T`           | Cycle delta threshold forward / backward (both mode)                           |
| `e`                 | Diagnose infeasibility                                                         |
| `w`                 | Write diff to CSV (both mode)                                                  |
| `y`                 | Yank solve results to clipboard                                                |
| `Esc`               | Close solver overlay                                                           |

**Rewrite & Diagnostics**

| Key     | Action                                                              |
| ------- | ------------------------------------------------------------------- |
| `E`     | What-if: edit the selected constraint's RHS and re-solve            |
| `P`     | Rewrite: pick presolve rules, then compare original vs rewritten    |
| `D`     | Diagnostics: why the solve is slow, and which rows/variables to blame |

**Export**

| Key | Action                         |
| --- | ------------------------------ |
| `w` | Export full diff report as CSV |

**Other**

| Key       | Action                                                     |
| --------- | ---------------------------------------------------------- |
| `r`       | Toggle raw text side-by-side view (constraints/objectives) |
| `s`       | Cycle sort mode (name → \|Δ\| → relΔ)                      |
| `t` / `T` | Cycle relative / absolute tolerance (rebuilds the diff)    |
| `?`       | Toggle help pop-up                                         |
| `q`       | Quit                                                       |
| `Ctrl+C`  | Force quit                                                 |

**Mouse:** the scroll wheel navigates lists and clicking selects entries.

## HiGHS Solver

Press `S` to solve either file on demand using the [HiGHS](https://highs.dev) solver. Pick file 1 or 2, and the solver runs in a background thread. Results are organised into tabs — Summary, Variables, Constraints, Log, and Duals — switchable with `1`–`5` or `Tab`/`Shift+Tab`. The Summary tab shows optimisation status, objective value, and solve time. Press `e` to run an infeasibility diagnosis when a problem does not solve.

Option 3 ("Both") solves both files and shows a side-by-side comparison. Rows are marked as "changed" when their absolute difference exceeds a configurable delta threshold. Press `t` to cycle forward through preset thresholds (`0.0`, `0.0001`, `0.001`, `0.01`, `0.1`, `1.0`) and `T` to cycle backward. The default threshold is `0.0001`. Press `d` to toggle between showing all rows and changed-only rows, and `w` to export the diff to CSV.

## What-if (`E`)

Select a constraint and press `E` to edit its right-hand side. The baseline problem (file 1) is cloned in memory, the
RHS is changed, and both versions are solved in parallel into the standard comparison view. Nothing is written to disk.

The comparison label records the change (`capacity rhs 200 -> 260`), so the result carries its own provenance. A shadow
price tells you the marginal value of a constraint at the current solution but not how far that stays true; moving the
bound and re-solving does.

## Rewrite (`P`)

`P` opens a picker of solution-preserving rewrites. Each removes work from the model without changing the set of optimal
solutions. Space toggles a rule, `a` toggles all, and `Enter` rewrites the baseline and launches an
original-vs-rewritten comparison solve. `w` writes the rewritten model to `<file>_presolved.lp` in the working directory
instead of solving it.

Paired solves run **one at a time**: the timings are the point of the comparison, and solving both at once would have
them compete for the machine and mask the speed-up.

| Rule                       | Effect                                                                            |
| -------------------------- | --------------------------------------------------------------------------------- |
| Fixed columns -> rhs       | A fixed variable's term is a constant: it moves to the rhs and the non-zero goes   |
| Singleton rows -> bounds   | A row with one term is a bound: `3x <= 12` becomes `x <= 4`, and the row goes      |
| Bound propagation          | Implied bounds derived from each row's minimum and maximum activity                |
| Integer bound rounding     | Fractional bounds on integer variables rounded inwards: `x <= 3.7` becomes `x <= 3` |
| Redundant & forcing rows   | Drops rows that can never bind; fixes variables pinned by a forcing row            |
| Empty rows & columns       | Drops termless rows; fixes variables appearing in no row at their preferred bound  |
| Row scaling                | Divides each row by a power of two so its largest coefficient sits near 1           |
| Column scaling             | Rescales each continuous variable's units by a power of two, largest coefficient near 1 |

The rules feed each other, so they run to a fixpoint (at most 10 passes) rather than once each. Reopening the picker
shows the previous run's per-pass breakdown.

The last three rules target what the diagnostics pane ranks rather than the row count. **Fixed columns -> rhs** is the
one that thins the densest rows: the other rules fix columns but leave their now-constant terms in the matrix, and each
new fix feeds back through this rule on the next pass.

**Row and column scaling** equilibrate the matrix, and they only work as a pair. A single factor per row cannot improve
the worst-conditioned *rows*, because a row's own max-to-min ratio is scale-invariant; it takes a different factor per
column to change it. Both factors are powers of two, so every mantissa survives and the rewrite adds no rounding error
of its own. Scaling never fires on a row or column already within a factor of two of 1 (which is what makes the fixpoint
terminate), and it skips anything that would sink a coefficient into the zero tolerance. Column scaling applies to
continuous variables only: integrality, binariness, the semi-continuous rule and SOS weights are all statements about a
variable's own units.

Scaling is the one rewrite that leaves the model in different units, and it is undone before you see it. The factors are
kept with the run and applied to the rewritten side's solve result on the way back: variable values, reduced costs, row
activities and shadow prices are all reported in the original model's units, so the comparison diffs like for like. The
objective value is invariant under both scalings anyway.

Measured on the bundled fixtures (`boeing2.lp`): worst column ratio 3.5e4 -> 3.9e3, at the cost of the worst row ratio
moving 3.0e3 -> 3.9e3. Equilibration is a redistribution, not a free win.

The one place the units *do* escape is `w`: a file on disk has nothing to unscale it, so a model written after a scaling
rule fired is in rewritten units, and the status line says so.

Rows are removed; columns are only ever *fixed*, never removed. Keeping the variable set identical on both sides is what
makes the comparison trustworthy: every variable still appears in both results, so a difference in the diff is real and
not an artefact of the rewrite. The objective values must agree, and that agreement is the check that the rewrite was
sound.

## Diagnostics (`D`)

`D` answers why a solve is taking so many iterations, and which rows and variables are responsible. Every constraint and
variable gets one record carrying both its structure (density, coefficient spread, bound width) and its behaviour in the
last solve (activity, shadow price, whether it sat at a bound with a zero dual).

The pane leads with a verdict, then the solver's own telemetry parsed from its log (iterations, HiGHS presolve
reductions, run time, primal-dual objective error), then the model's magnitude ranges, then ranked tables for
worst-conditioned, degenerate and densest, rows and columns each.

Three signals are kept separate because they have different cures:

- **Conditioning** — a row spanning many orders of magnitude makes the ratio test pick badly. Cured by scaling.
- **Density** — a dense row or column destroys basis-factorisation sparsity. It makes each iteration *cost* more; it does
  not make the solver take more of them.
- **Degeneracy** — many constraints active at the same vertex, so the simplex shuffles between bases without improving
  the objective. The usual cause of a runaway iteration count, and scaling will not fix it.

The verdict judges on iterations *per row*, not the raw count: 200k iterations is unremarkable at 100k rows and alarming
at 500. Degeneracy is checked before conditioning, since it is the more common cause and the one where rescaling is
wasted effort.

The degeneracy figures are proxies computed from the final solution, not a basis inspection (HiGHS does not expose the
basis through this binding). The pane says so on screen. Solver telemetry is parsed from the log, which is not an API:
every field is optional, so a format change leaves a gap rather than breaking the pane.

## Jumplist

Navigation positions are recorded automatically when you change sections, apply filters, or jump to a search result. Use `Ctrl+o` to go back and `Ctrl+i` to go forward through your navigation history (up to 100 positions).

## Colour Scheme

- **Green** `[+]` — Added entries
- **Red** `[-]` — Removed entries
- **Yellow** `[~]` — Modified entries

## Requirements

Requires a terminal with colour support (most modern terminals).
