# lp_diff: LP/MPS model explorer and diff viewer

A terminal explorer and diff viewer for linear programming models in LP and MPS format, built with [ratatui](https://ratatui.rs). Pass one file to explore a single model, or two files to diff them.

![lp_diff demo](assets/demo.gif)

The demo runs two scenes. First it diffs `fit2d.mps` against `fit1d.lp` (mixed formats), touring the coefficient-level constraint diff, the raw text view, live tolerance cycling, fuzzy search, Numerics, and a HiGHS solve of both files with a solution diff. Then it inspects `boeing2.lp` on its own to apply the presolve rewrites (`P`) and open the diagnostics pane (`D`).

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

Both LP and MPS files are accepted, and the two sides of a diff may be in different formats:

```sh
lp_diff model.lp model.mps
```

With one file the viewer opens a single-model explorer (inspect mode); with two it computes a diff and opens the diff viewer. The format is chosen by extension: `.mps` (any case) is read as MPS, anything else as LP.

### Options

| Flag | Effect |
| --- | --- |
| `--summary` | Print a text summary to stdout and exit without opening the TUI (see below). |
| `--watch` | Re-parse and rebuild when an input file changes on disk. A change is picked up once the file's modification time has been stable for two polls (about half a second), so a file written in chunks is not read half-finished. |
| `--abs-tol <X>` | Absolute tolerance for comparing coefficients and right-hand sides: values within `X` compare equal. Default `0`; a floor of `1e-10` always applies. |
| `--rel-tol <X>` | Relative tolerance: values within `X * max(\|a\|, \|b\|)` compare equal. Default `0`. |
| `--rename <PATTERN> <REPLACEMENT>` | Regex rewrite applied to names in both files before they are matched. Repeatable; rules apply in order. `--rename '\[\d+\]' '[i]'` matches `x[1]` and `x[2]` as the same name. |
| `--theme <auto\|dark\|light>` | Colour palette. `auto` (the default) reads `COLORFGBG`, then asks the terminal for its background colour, and falls back to dark. With `auto`, setting `NO_COLOR` gives a monochrome palette. |

The tolerance and rename options only affect diff mode. Invalid values are rejected before any file is parsed.

Exit codes: `0` on success, `1` for a runtime error (missing file, bad option value, parse failure, terminal error), `2` for a command-line usage error.

### Inspect Mode (single file)

Given one file, the same five sections describe that model rather than a comparison:

- Summary: file path, problem name, sense, per-section counts, and the structural analysis (dimensions, variable/constraint types, coefficient scaling, issues).
- Variables, Constraints, Objectives: every entry in the model, listed plainly (no diff badges). The detail panel shows the full entry: coefficients with names, operator and RHS, bounds and variable type, or SOS weights.
- Numerics: the numerical conditioning view.

Search (`/`), the command palette (`Ctrl+p`), the HiGHS solver (`S`, which solves the model directly with no file picker), the analyses (`E`, `P`, `D`, `B`, `U`, `I`, `R`), CSV export (`w`) and `--watch` all work. Diff-only actions do nothing but show a status-bar hint: the kind filters (`a`/`+`/`-`/`m`/`=`), ignore-order (`o`), sort cycling (`s`), tolerance cycling (`t`/`T`), the raw side-by-side view (`r`) and the per-file yanks (`yo`/`yn`). The help pop-up and the palette leave them out.

### Summary Mode

`--summary` prints a text report to stdout and exits. In diff mode it prints the change counts; in inspect mode it prints the model's size and up to 20 analysis issues:

```sh
lp_diff base.lp modified.lp --summary   # diff summary
lp_diff model.lp --summary              # single-model summary
```

Diff output (`+` added, `-` removed, `~` modified, `>` renamed; an `Options:` line follows the header when a tolerance or rename rule is set):

```text
LP Diff: base.lp vs modified.lp

Variables:    +3   -1   ~2   >0   (42 unchanged)
Constraints:  +0   -5   ~12  >1   (300 unchanged)
Objectives:   +0   -0   ~1   >0   (0 unchanged)

Renamed: 1 (counted once, excluded from added/removed)
Total: 25 changes
```

## Layout

The screen has a tab bar, two panels and a status bar:

| Panel            | Description                                                                             |
| ---------------- | --------------------------------------------------------------------------------------- |
| Section tabs     | Tab bar across the top: Summary, Variables, Constraints, Objectives, Numerics           |
| Name List        | Left sidebar: filterable list of entries in the selected section (on Summary and Numerics, an overview whose rows open their section when clicked) |
| Detail           | Right panel: full detail for the selected entry                                         |

The status bar at the bottom shows total changes, per-section diff statistics (`+N -N ~N`), the active filter, and scroll position.

Press `?` to open the key bindings pop-up. It lists only the keys that apply to the current mode.

### Sections

| #   | Section     | Description                                                                       |
| --- | ----------- | --------------------------------------------------------------------------------- |
| 1   | Summary     | Overview of change counts, problem dimensions, and structural analysis            |
| 2   | Variables   | Variable kind and bound changes                                                   |
| 3   | Constraints | Constraint changes with coefficient-level detail (side-by-side view for modified) |
| 4   | Objectives  | Objective function changes                                                        |
| 5   | Numerics    | Per-file numerical conditioning view (coefficient scaling, ranges, issues)        |

### Side-by-Side Constraint View

Modified standard constraints are displayed as a table with the old and new coefficients side by side, aligned on their decimal points. Modified coefficients also show the change (`Δ`) and relative change (`%Δ`); a side a coefficient is missing from shows a dimmed `—`. Added coefficients are highlighted in green, removed in red, and modified in yellow. Unchanged coefficients appear in grey. On a narrow pane the `%Δ` and then `Δ` columns are dropped before the names are shortened.

### Raw Text View

Press `r` to toggle between the parsed diff and a side-by-side view of the source lines for the selected constraint or objective, file 1 on the left and file 2 on the right.

### CSV Export

In diff mode, press `w` to export the diff as `lp_diff_report_<timestamp>.csv` in the current directory, with columns for section, name, change type and detail. The timestamp is Unix seconds. In inspect mode, `w` exports the model itself as `objectives.csv`, `constraints.csv`, and `variables.csv` (via the core library's `to_csv`) into a new `<file stem>_csv_<timestamp>` folder, so an export never overwrites earlier files. The status bar shows the full path written.

### Key Bindings

**Navigation**

| Key          | Action                    |
| ------------ | ------------------------- |
| `j` / `↓`    | Move down                 |
| `k` / `↑`    | Move up                   |
| `n`          | Next search match         |
| `N`          | Previous search match     |
| `g` / `Home` | Jump to top               |
| `G` / `End`  | Jump to bottom            |
| `Ctrl+d`     | Half page down            |
| `Ctrl+u`     | Half page up              |
| `Ctrl+f`     | Full page down            |
| `Ctrl+b`     | Full page up              |
| `Ctrl+o`     | Jump back (jumplist)      |
| `Ctrl+i`     | Jump forward (jumplist)   |
| `Tab` / `Shift+Tab` | Switch between the list and the detail panel |
| `Enter`      | Go to detail panel        |
| `h` / `l`    | Move to sidebar / detail  |
| `1`–`5`      | Jump to section by number |
| `[` / `]`    | Previous / next section   |
| `<` / `>`    | Narrow / widen the sidebar |
| `M`          | Toggle mouse capture (off: select text with the terminal) |
| `Esc`        | Back / clear search       |
| `Ctrl+p`     | Command palette: fuzzy-find any action |
| `Ctrl+z`     | Suspend to the shell (Unix) |

**Filters**

| Key | Action                                   |
| --- | ---------------------------------------- |
| `a` | All changes                              |
| `+` | Added only                               |
| `-` | Removed only                             |
| `m` | Modified only                            |
| `=` | Renamed only                             |
| `o` | Toggle ignore-coefficient-order matching |

**Search pop-up**

| Key       | Action                                                       |
| --------- | ------------------------------------------------------------ |
| `/`       | Open search pop-up (searches across all sections)            |
| `↓` / `Ctrl+n` / `Ctrl+j` | Next result (plain `j`/`k` are typed into the query, since names often contain them) |
| `↑` / `Ctrl+p` / `Ctrl+k` | Previous result                                  |
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
| `c:`     | Substring over entry content (variables, coefficients, RHS) rather than names |

**Clipboard**

| Key  | Action                                          |
| ---- | ----------------------------------------------- |
| `yy` | Yank selected entry name to clipboard           |
| `yo` | Yank old (file 1) version of entry to clipboard |
| `yn` | Yank new (file 2) version of entry to clipboard |
| `Y`  | Yank full detail panel content to clipboard     |

Over SSH or inside tmux (`SSH_TTY` or `TMUX` set), yanks go through the terminal as an OSC 52 escape instead, so they land on the clipboard of the machine you are sitting at; locally, OSC 52 is the fallback when the system clipboard is unavailable. The status bar says which route was taken. Under tmux, OSC 52 needs `set -g set-clipboard on`.

**Solver**

| Key                 | Action                                                                         |
| ------------------- | ------------------------------------------------------------------------------ |
| `S`                 | Solve problem with HiGHS                                                       |
| `1` / `2`           | Select file 1 or file 2 (in picker)                                            |
| `3`                 | Solve both and diff (in picker)                                                |
| `1`–`5`             | Switch tab: Summary / Variables / Constraints / Log / Duals (in results view) |
| `Tab` / `Shift+Tab` | Cycle result tabs forward / backward                                           |
| `j` / `k`           | Scroll results (in results view)                                               |
| `d`                 | Toggle diff-only filter (both mode)                                            |
| `t` / `T`           | Cycle delta threshold forward / backward (both mode)                           |
| `e`                 | Diagnose infeasibility (elastic relaxation)                                    |
| `I`                 | Irreducible infeasible subsystem of the model on screen                        |
| `w`                 | Write results to CSV (the comparison, in both mode)                            |
| `y`                 | Yank solve results to clipboard                                                |
| `Esc`               | Close solver overlay (the result is cached until the input changes); while solving, interrupt HiGHS |
| `q`                 | While solving, ask before quitting (`y` quits)                                 |

**Analyses**

| Key     | Action                                                              |
| ------- | ------------------------------------------------------------------- |
| `E`     | What-if: edit the selected constraint's RHS and re-solve            |
| `P`     | Rewrite: pick presolve rules, then compare original vs rewritten    |
| `l`     | (in the `P` picker) Log what the rewrite removes; `w` writes it to a file |
| `H`     | (in the `P` picker) Log what HiGHS's own presolve removes from the file  |
| `D`     | Diagnostics: why the solve is slow, and which rows/variables to blame |
| `B`     | Solve profile: the model under several HiGHS configurations        |
| `U`     | Unbounded ray: which variables run away                             |
| `I`     | Irreducible infeasible subsystem: a minimal conflicting set         |
| `R`     | Ranging: how far each objective coefficient and row activity can move before the basis changes |

`B`, `U`, `I` and `R` run on a background thread; any key other than a scroll key closes the pane (cancelling a run in progress), and `w` writes the finished report to `<stem>_solve_profile.txt`, `<stem>_unbounded_ray.txt`, `<stem>_iis.txt` or `<stem>_ranging.txt`, where `<stem>` is file 1's name without its extension.

**Export**

| Key | Action                         |
| --- | ------------------------------ |
| `w` | Export the diff (or, in inspect mode, the model) as CSV |

**Other**

| Key       | Action                                                     |
| --------- | ---------------------------------------------------------- |
| `r`       | Toggle raw text side-by-side view (constraints/objectives) |
| `s`       | Cycle sort mode (name → \|Δ\| → relΔ)                      |
| `t` / `T` | Cycle relative / absolute tolerance through `0`, `1e-9`, `1e-6`, `1e-4`, `1e-2` (rebuilds the diff; a non-preset CLI value restarts at `0`) |
| `?`       | Toggle help pop-up                                         |
| `q`       | Quit                                                       |
| `Ctrl+C`  | Force quit                                                 |

Mouse: the scroll wheel navigates lists and a click selects an entry. To select text, hold Shift while dragging (Option in macOS terminals), or press `M` to release the mouse.

## HiGHS Solver

Press `S` to solve a model with [HiGHS](https://highs.dev). In diff mode you pick file 1, file 2 or both; in inspect mode the one model is solved straight away. The solve runs on a background thread. Results are split into tabs (Summary, Variables, Constraints, Log, Duals), switchable with `1`–`5` or `Tab`/`Shift+Tab`. The Summary tab shows the status, objective value and solve time. Press `e` to run an infeasibility diagnosis when a problem does not solve.

If a `highs.opt` file exists in the current directory, its options are applied to every solve. It uses the same `key = value` format as the `highs` command's `--options_file`, with `#` comments. `log_file` and `output_flag` are ignored because the viewer sets them itself, and an option HiGHS rejects is noted in the Log tab rather than failing the solve.

Option 3 ("Both") solves both files and shows a side-by-side comparison. A row is marked as changed when its absolute difference exceeds the delta threshold. Press `t` to cycle forward through preset thresholds (`0.0`, `0.0001`, `0.001`, `0.01`, `0.1`, `1.0`) and `T` to cycle backward. The default threshold is `0.0001`. Press `d` to toggle between showing all rows and changed-only rows, and `w` to export the diff to CSV.

## What-if (`E`)

Select a constraint and press `E` to edit its right-hand side. The baseline problem (file 1) is cloned in memory, the
RHS is changed, and both versions are solved, one after the other, into the standard comparison view. Nothing is written to disk.

The comparison label records the change (`capacity rhs 200 -> 260`), so the result carries its own provenance. A shadow
price tells you the marginal value of a constraint at the current solution but not how far that stays true; moving the
bound and re-solving does.

## Rewrite (`P`)

`P` opens a picker of rewrites applied to the baseline model (file 1). All but the last two remove work from the model
without changing the set of optimal solutions. Space toggles a rule, `a` toggles all, and `Enter` rewrites the baseline
and launches an original-vs-rewritten comparison solve. `w` writes the rewritten model, always in LP format, to
`<stem>_presolved.lp` in the working directory instead of solving it (`<stem>` is file 1's name without its extension).
`l` opens the log of what the rewrite did, and `H` the log of what HiGHS's own presolve does to the untouched file.

Paired solves run one at a time: the timings are the point of the comparison, and solving both at once would have
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
| Split dense rows (what-if) | An `n`-term row becomes `sqrt(n)` partial sums plus one aggregate row. **Off by default** |
| Relax integrality (what-if) | Integer and binary columns become continuous: the LP relaxation. **Off by default** |

The rules feed each other, so they run to a fixpoint (at most 10 passes) rather than once each. Reopening the picker
shows the previous run's per-pass breakdown.

The last two rules are what-ifs: every other rule preserves the set of optimal solutions, and these two break that on
purpose, which is why both start unticked. The comparison is their whole output. How much a structural change is worth
is a question about your model and your solver, and the only honest way to answer it is to run both sides.

Split dense rows is the one rule that grows the model. A row of `n` terms becomes `k = ceil(sqrt(n))` defining equalities `part_i - sum(chunk_i) = 0` plus an aggregate
`sum(part_i) <op> rhs`, so the worst row density falls from `n` to about `sqrt(n)` for about `sqrt(n)` extra rows,
columns and non-zeros — at `n = 1728` that is 1728 down to ~42 for a ~2% rise in non-zeros. A running-total chain
reaches the same density for `n` extra rows and columns and roughly three times the non-zeros, so it is only worth it
when the cumulative quantity is itself wanted. Only rows of at least 64 non-zeros are touched; below that the aggregate
row is no sparser than the chunks it aggregates.

None of which means it is faster. The simplex factorises the basis and updates it, so a dense row costs it almost
nothing and the extra rows and columns are pure overhead: against `HiGHS`'s default expect neutral to slightly worse.
The density collapse pays off for interior-point methods, where row density lands in the normal equations and squares.
Enable the rule, press Enter, and read the comparison — that is what it is for. The partial sums are new columns, so
they show as added rows in the comparison; every original variable still lines up.

Relax integrality deletes the integrality constraints, so the relaxed objective is a bound on the original rather
than equal to it. On a pure LP it does nothing. The point is the pair of numbers the comparison then shows: the
objective difference is the integrality gap (what optimality costs over the bound), and the time difference is how much
of the run was branch-and-bound rather than simplex. A model that relaxes in milliseconds and takes minutes as a MIP has
a branching problem, not a linear-algebra one, and no amount of scaling or row thinning will touch it. Bounds are
materialised while relaxing, because a binary column's `[0, 1]` is implied by its kind rather than written down;
relaxing without it would turn `b in {0, 1}` into `b >= 0`. Semi-continuous and SOS columns keep their kind: those are
not integrality.

Three of the rules target what the diagnostics pane ranks rather than the row count. Fixed columns -> rhs is the
one that thins the densest rows: the other rules fix columns but leave their now-constant terms in the matrix, and each
new fix feeds back through this rule on the next pass.

Row and column scaling equilibrate the matrix, and they only work as a pair. A single factor per row cannot improve
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
not an artefact of the rewrite. With only solution-preserving rules ticked the objective values must agree, and that
agreement is the check that the rewrite was sound.

### Presolve log (`l`)

The per-pass counters say how much the rewrite did; `l` says *to what*. It runs the ticked rules and opens a scrollable
record of every action in the order it happened — each row dropped and why, each column fixed and to what value, each
bound moved, each row or column rescaled, and the verdict if a rule proved the model infeasible:

```
pass 1  singleton   c2                       row removed -> x <= 4
pass 1  bound       x                        [0, 10] -> [0, 4]
pass 1  redundant   c3                       row removed, activity in [0, 14] vs rhs 900
pass 1  unused col  z                        [0, 5] -> [0, 0]
```

`w` in the log pane writes it to `<stem>_presolve_log.txt` in the working directory. The log is capped at 20,000 lines
(the counters always cover the whole run); a truncated log says so on its last line.

### What HiGHS's own presolve removes (`H`)

`l` reports our rewrite. `H` reports the solver's, on the file exactly as written with none of our rules applied — which
rows and columns HiGHS throws away before it runs a single simplex iteration:

```
HiGHS presolve: -0 rows, -2 cols, 24 rows x 1024 cols x 13386 nnz left, 17.6ms
24 rows, 1026 cols in → 24 rows, 1024 cols out

2 column(s) removed
  R0100146
  R0200146
```

Unlike our rewrite, HiGHS removes columns as well as rows, so the two are listed separately. `w` writes the report to
`<stem>_highs_presolve.txt`. Pressing `l` with no rules ticked lands here too:
with nothing selected our rewrite has nothing to say, and the solver's answer is the useful one.

The names are real HiGHS output, not an inference: `HPresolve::shrinkProblem` carries row and column names through the
reduction, and the C API hands the survivors back (`Highs_getPresolvedColName` / `Highs_getPresolvedRowName`). The
`highs` crate never passes names in, so this module reaches past it to `highs-sys` for those calls; the model itself is
built by the same code the ordinary solve uses, so the report is about the model the solver actually sees. Presolve
proving the model infeasible is reported as such rather than as "everything was removed".

## Diagnostics (`D`)

`D` answers why a solve is taking so many iterations, and which rows and variables are responsible. Every constraint and
variable gets one record carrying both its structure (density, coefficient spread, bound width) and its behaviour in the
last solve (activity, shadow price, whether it sat at a bound with a zero dual).

The pane leads with a verdict, then the solver's own telemetry parsed from its log (iterations, HiGHS presolve
reductions, run time, primal-dual objective error), then the model's magnitude ranges, then ranked tables for
worst-conditioned, degenerate and densest, rows and columns each.

Three signals are kept separate because they have different cures:

- Conditioning: a row spanning many orders of magnitude makes the ratio test pick badly. Cured by scaling.
- Density: a dense row or column destroys basis-factorisation sparsity. It makes each iteration *cost* more; it does
  not make the solver take more of them.
- Degeneracy: many constraints active at the same vertex, so the simplex shuffles between bases without improving
  the objective. The usual cause of a runaway iteration count, and scaling will not fix it.

The verdict judges on iterations *per row*, not the raw count: 200k iterations is unremarkable at 100k rows and alarming
at 500. Degeneracy is checked before conditioning, since it is the more common cause and the one where rescaling is
wasted effort.

The degeneracy figures are proxies computed from the final solution, not a basis inspection (HiGHS does not expose the
basis through this binding). The pane says so on screen. Solver telemetry is parsed from the log, which is not an API:
every field is optional, so a format change leaves a gap rather than breaking the pane. Without a solve the pane still
shows the structural tables and says there is no solve to judge.

## Model queries (`B`, `U`, `I`, `R`)

These run on the baseline model (file 1) on a background thread and open a scrollable report.

- `B`, solve profile: solves the model under six configurations one after another (HiGHS defaults, presolve off, dual
  simplex, primal simplex, interior point, and our rewrite followed by a default solve) and tabulates time, iterations
  and objective. They run sequentially because parallel solves would compete for the cores and distort the times being
  compared. Each preset after the first is capped at ten times the baseline's time (at least 10 s), and a preset that
  reaches a different objective is flagged.
- `U`, unbounded ray: for an unbounded model, the variables with a non-zero component in HiGHS's ray of unboundedness.
- `I`, irreducible infeasible subsystem: a set of constraints and bounds that cannot all hold, and none of whose proper
  subsets is infeasible. This is a different question from `e` in the solver overlay, which finds the cheapest set of
  constraints to relax. From the solver overlay, `I` runs on the model whose result is shown.
- `R`, ranging: for an optimal solve, the interval over which each objective coefficient and row activity can move
  before the optimal basis changes. The entry selected when `R` was pressed is shown in full at the top.

Unbounded rays, IIS and ranging are LP concepts, so for a MIP they are computed on the LP relaxation and the report says
how many columns were relaxed.

## Jumplist

Navigation positions are recorded automatically when you change sections, apply filters, or jump to a search result. Use `Ctrl+o` to go back and `Ctrl+i` to go forward through your navigation history (up to 100 positions).

## Colour Scheme

| Badge | Colour | Meaning |
| --- | --- | --- |
| `[+]` | green | Added |
| `[-]` | red | Removed |
| `[~]` | yellow (orange on a light background) | Modified |
| `[>]` | accent | Renamed |

With `NO_COLOR` set the badges are the only marker.

## Requirements

A terminal with colour support. On terminals that support the kitty keyboard protocol (kitty, WezTerm, Ghostty, iTerm2, foot, Windows Terminal) `Ctrl+i` is told apart from `Tab`; elsewhere the two are the same key.
