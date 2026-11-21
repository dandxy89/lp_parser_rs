# lp-lsp

A language server for LP (linear programming) files. It understands the CPLEX LP format and the Gurobi
extensions that [`lp_parser_rs`](../rust) supports, and it keeps working while a file is half-written.

## What it does

- **Diagnostics**: as you type, syntax errors and structural checks (duplicate names, a variable in more than
  one type section, conflicting bounds, unused declarations, `=<`/`=>` spellings); after a short pause, errors
  from the full parse and the model analysis. Analysis findings have a stable code, a category (structural,
  numerical, mathematical or performance) and a certainty: proven findings are always published, while heuristic
  ones (smells that are sometimes intended, such as badly scaled coefficients) can be switched off with
  `lp.analysis.heuristics`. Solver and IIS findings appear once a solve has run. Both push and pull diagnostics
  are supported, including workspace diagnostics for `*.lp` files that are not open.
- **Navigation**: go to definition, declaration (a variable's type-section entry) and type definition, find
  references, highlights, linked editing of a name, call hierarchy (which constraints use a variable, and which
  variables a constraint uses), and document and workspace symbols.
- **Rename**: a variable within its file, or a constraint, objective or SOS name across every `*.lp` file in the
  workspace. It refuses names that would change how the file parses.
- **Hover and completion**: details on variables, constraints and keywords (with any `\ @description` comment
  written directly above the entry), and suggestions that fit where the cursor is.
- **Formatting**: whole document, a selection, or on Enter. It never touches a file with syntax errors.
- **Code actions**: quick fixes for the problems it reports, plus refactors such as adding a bound, moving a
  variable between type sections and reordering sections.
- **Extras**: inlay hints, semantic highlighting, folding, selection ranges, code lens (variable counts on
  constraints and objectives, usage counts on variables, and, with `codeLens.actions`, Solve on the objective and
  Explain on each named constraint) and signature help for `MAX`, `MIN`, `ABS`, `AND` and `OR`.
- **Commands** (`workspace/executeCommand`): `lp.analyze` (analysis report), `lp.convertToMps` (writes
  `<name>.mps` next to the file, replacing any existing one) and `lp.showModelStats`.
- **MPS conversion**: `lp.convertFromMps` (arguments: URI and the MPS text, since MPS buffers are not synchronised) returns `{ lp, omitted }` without writing a file; `omitted` lists empty rows that LP syntax cannot hold.
- **Model and solver requests**: custom `lp/*` requests for editor views (model explorer, coefficient matrix,
  dependency graph, mathematical diff, explanations) and for solving with HiGHS. See
  [Custom requests](#custom-requests).

The server indexes every `*.lp` file under the workspace folders at start-up, skipping `.git`, `target`,
`node_modules`, `.venv`, `venv` and `__pycache__`, and stopping once the indexed text reaches 256 MB. It keeps the
index current through file watching or, for clients without it, file-operation notifications.

### Descriptions

A `\ @description` comment directly above an entry documents it. Hover shows it, and `lp/model` and `lp/explain`
return it as `description`.

```lp
Subject To
 \ @description Capacity of the north plant
 c1: x + y <= 10
```

- A description line is `\`, optional blanks, `@description` (case-sensitive), then blanks and text. It must be a
  real line comment, not inside a `\* ... *\` block. CRLF is fine.
- Consecutive description lines directly above the entry join with single spaces (at most 16 lines, the nearest
  are kept). A blank line or a plain comment between them and the entry detaches the description.
- It attaches to an objective, constraint, general constraint or SOS set that starts its line, and to a variable's
  first `Bounds` or type-section entry on its line (`b1` of `b1 b2 b3`). A description above a constraint is not
  passed to the variables it uses, and a trailing comment on the entry's own line is never a description.
- Control and bidirectional-control characters are removed and the text is cut at 1000 characters (`…`).
  Hover escapes Markdown, so a description cannot add links, images or HTML.

## Install

Prebuilt `lp-lsp` archives for Linux, macOS and Windows (x86_64 and aarch64), with SHA-256 checksums, are attached to each [`lp-lsp-v*` GitHub release](https://github.com/dandxy89/lp_parser_rs/releases).

Or build from crates.io:

```bash
cargo install --locked lp-lsp
lp-lsp --version
```

The server talks LSP over stdio. It takes no arguments other than `--version`.

Notes:

- Positions use UTF-8 when the client offers it, otherwise UTF-16.
- Files over 1 MiB get syntax diagnostics only after the debounce.
- Pull diagnostics are advertised only when the client declares support; in pull mode push is disabled.
- Indexing does not follow symlinked directories and warns on non-UTF-8 files.
- Unknown settings keys are ignored; an invalid value rejects the whole new configuration, keeps the previous one and shows an error.
- A lone `\r` is not treated as a line break.

## Settings

Everything lives under `lp` and is optional.

| Setting | Default | |
| --- | --- | --- |
| `analysis.enabled` | `true` | Publish analysis diagnostics. |
| `analysis.heuristics` | `true` | Publish heuristic findings. Proven findings are unaffected. |
| `analysis.categories.structural` / `numerical` / `mathematical` / `performance` | `true` | Switch each diagnostic category on or off. |
| `analysis.severityOverrides` | `{}` | Per-code severity, keyed by code (for example `"large-coefficient": "hint"`; values are `error`, `warning`, `information`, `hint` or `off`). `off` hides the code, and any other value shows it whatever `heuristics` and `categories` say. |
| `analysis.largeCoefficientThreshold` | `1e9` | Flag larger coefficients. |
| `analysis.smallCoefficientThreshold` | `1e-9` | Flag smaller non-zero coefficients. |
| `analysis.largeRhsThreshold` | `1e9` | Flag larger right-hand sides. |
| `analysis.coefficientRatioThreshold` | `1e6` | Flag a larger max/min coefficient ratio. |
| `analysis.rhsCoefficientRatioThreshold` | `1e9` | Flag a right-hand side larger than this multiple of the largest coefficient in its row. |
| `analysis.feasibilityTolerance` | `1e-6` | Slack before a bound or row violation is reported as proven. |
| `analysis.equalityTolerance` | `1e-9` | Relative tolerance for two normalised coefficients to count as equal. |
| `analysis.maxPerCode` | `1000` | Diagnostics published per code before the rest are summarised. |
| `solver.path` | empty | Path of the `lp-highs` helper. Empty searches next to `lp-lsp`, then `PATH`. |
| `solver.timeLimit` | `60` | Wall-clock limit for one solve, in seconds. |
| `solution.inlayHints` | `true` | Show solution values as inlay hints after a solve. |
| `solution.showStale` | `false` | Keep showing a solution after the document has changed. |
| `semantic.debounceMs` | `300` | Wait this long after typing before the full check. |
| `semantic.maxFileSizeMb` | `20` | Above this size, skip the keystroke-time full check; the semantic pass still runs on open, save and configuration change. |
| `format.indent` | `2` | Indent in spaces (0–16). |
| `format.lineWidth` | `100` | Wrap long lines (at least 20). |
| `format.alignOperators` | `false` | Line up comparison operators. |
| `format.keywordCase` | `"preserve"` | `preserve`, `lower`, `upper` or `title`. |
| `inlayHints.generatedNames` | `true` | Names for unnamed constraints and objectives. |
| `inlayHints.rangePartners` | `true` | The `_rng` partner of ranged constraints. |
| `inlayHints.variableTypes` | `true` | A variable's type after its first use. |
| `inlayHints.normalisedRhs` | `true` | The right-hand side with left-hand-side constants folded in. |
| `codeLens.actions` | `false` | Solve and Explain lenses. They run the VS Code extension's lp.solve and lp.explainConstraint, so other clients should leave this off. |

## Custom requests

The server advertises `experimental.lp` in its initialize result: `protocol` (currently `1`), `requests`,
`notifications` and `solver` (whether an `lp-highs` helper was found). Clients that do not know these requests
ignore them, and the standard features work without them.

| Request | Purpose |
| --- | --- |
| `lp/model` | Model Explorer data: objectives, constraints, variables grouped by type, and special rows, with counts and source ranges, and each entry's `description`. |
| `lp/explain` | What an objective, constraint or variable means in the model's own terms: terms, bounds, activity range, related rows and diagnostics, and each entry's `description`. |
| `lp/graph` | The bipartite row/variable dependency graph. |
| `lp/matrix` | The sparse coefficient matrix, as exact entries or a density raster for large models. |
| `lp/diff` | The mathematical difference between the buffer and another copy of the model. |
| `lp/solve` | Solves the document through the `lp-highs` helper. Cancellable; it reports progress as it runs. |
| `lp/solverInfo` | Whether the helper is available, its version and the HiGHS version it links. |

The notification `lp/solutionChanged` tells clients that the solution for a document has changed.

**Solving** needs the `lp-highs` helper (from `cargo install --locked lp-highs`). It is found through
`lp.solver.path`, then next to `lp-lsp`, then `PATH`. The server runs it as a separate process, so a solver crash
cannot take the language server down, and it kills the process on cancellation. HiGHS is the only solver:
CPLEX, Gurobi, CBC and GLPK are not supported by the editor.

## Editor setup

**Restricted mode.** A client that sets `initializationOptions.workspaceTrusted` to `false` (any non-boolean value counts as untrusted) gets `lp/solve` and `lp/solverInfo` that find `lp-highs` only next to `lp-lsp`; `solver.path` and `PATH` are ignored.

**VS Code**: build and install the extension in [`editors/vscode`](editors/vscode).

```bash
cd lsp/editors/vscode
npm ci && npm run package
code --install-extension lp-lsp-*.vsix
```

**Neovim (0.11+)**

```lua
-- ~/.config/nvim/lsp/lp_lsp.lua
return {
  cmd = { 'lp-lsp' },
  filetypes = { 'lp' },
  root_markers = { '.git' },
  settings = { lp = { format = { indent = 2 } } },
}
```

```lua
-- init.lua
vim.filetype.add({ extension = { lp = 'lp' } })
vim.lsp.enable('lp_lsp')
vim.lsp.inlay_hint.enable(true)
```

The "used in N constraints" code lens runs the client command `lp.showReferences` (arguments: URI,
position, locations). To send it to the quickfix list, add this to the config above:

```lua
commands = {
  ['lp.showReferences'] = function(command, ctx)
    local client = assert(vim.lsp.get_client_by_id(ctx.client_id))
    local items = vim.lsp.util.locations_to_items(command.arguments[3] or {}, client.offset_encoding)
    vim.fn.setqflist({}, ' ', { title = 'LP references', items = items })
    vim.cmd.copen()
  end,
},
```

**Helix**

```toml
# ~/.config/helix/languages.toml
[language-server.lp-lsp]
command = "lp-lsp"

[[language]]
name = "lp"
scope = "source.lp"
file-types = ["lp"]
comment-token = "\\"
roots = []
language-servers = ["lp-lsp"]
```

**Zed**: Zed needs an extension to register a language, so the simplest route is a dev extension containing
`extension.toml` and `languages/lp/config.toml` (`name = "LP"`, `path_suffixes = ["lp"]`,
`line_comments = ["\\ "]`). Then point Zed at the binary in `settings.json`:

```json
{
  "lsp": {
    "lp-lsp": {
      "binary": { "path": "lp-lsp" },
      "settings": { "lp": { "format": { "indent": 2 } } }
    }
  },
  "languages": { "LP": { "language_servers": ["lp-lsp"] } }
}
```

Any other editor works the same way: run `lp-lsp` for `*.lp` files.

## Not planned

These are deliberately left out of the language server:

- Solvers other than HiGHS (CPLEX, Gurobi, CBC, GLPK).
- Merging duplicate variables, extracting expressions, a model-generation assistant, workspace-wide model
  similarity, a sensitivity slider and a metrics history.

## Development

```bash
cargo test -p lp-lsp                            # solver paths use a stand-in helper
cargo build -p lp-highs && cargo test -p lp-lsp --features solver-tests   # also runs the real lp-highs helper
```
