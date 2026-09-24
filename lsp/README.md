# lp-lsp

A [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) implementation for
LP (linear programming) files: the CPLEX LP format plus the Gurobi extensions accepted by
[`lp_parser_rs`](../rust) (multi-objectives, general constraints, indicators, lazy constraints, user cuts,
quadratic terms, semi-continuous variables, SOS sets).

Syntax is handled by an incremental, error-tolerant [tree-sitter](https://tree-sitter.github.io/) grammar
(`tree-sitter-lp`), so every keystroke gets fresh diagnostics, symbols and highlighting even in broken files.
Semantics come from a debounced full `lp_parser_rs` parse and analysis, run off the main thread.

## Features

| Feature | What you get |
| --- | --- |
| **Diagnostics** | Syntax errors (`unexpected …` / `missing …`) on every keystroke. `lp_parser_rs` parse errors at the offending token, shown only when the file has no syntax errors. Duplicate constraint/objective/SOS names (with a link to the first). A variable listed in more than one type section. Bound or type declarations for unused variables. Conflicting bounds (lower > upper). `=<`/`=>` spelling. Analysis warnings (numerical scaling, empty constraints, fixed/unused variables…) placed on the entity they concern. Each diagnostic has a `code`. Push or pull (`textDocument/diagnostic`), whichever the client supports. |
| **Document symbols** | Sections → objectives, constraints, general constraints and SOS sets, with correct ranges. |
| **Workspace symbols** | Fuzzy search over constraint, objective, SOS and variable names across every `*.lp` file in the workspace. Files are indexed in the background with progress and kept current via file watching. |
| **Navigation** | Go to definition (a variable's `Bounds` entry, else its first use), declaration (its type-section entry), type definition. References honour `includeDeclaration`. Document highlights mark the definition as write and uses as read. |
| **Rename** | Variables and constraint/objective/SOS names; names shared across workspace files are renamed there too. Rejects names that would change parsing (a section keyword at line start, `free`, `S1`, `st`…), a leading digit, `inf`/`infinity`, invalid characters and collisions, with a clear message. The result is re-parsed as a final check. |
| **Hover** | Variables: type, bounds, objective coefficients, constraints (with coefficients) and SOS membership. Constraints: normalised form, section, `_rng` partner, indicator condition. Objectives: sense, attributes, term count. Keywords: description and every accepted alias. `MAX`/`MIN`/`ABS`/`AND`/`OR`: signature and semantics. |
| **Completion** | Section keywords (with snippets) only at the start of a line; variables in expressions, bounds, type sections and SOS entries; `S1`/`S2` in SOS headers; general-constraint functions after `r =`; objective attributes in `multi-objectives` files; comparison operators; `free` in `Bounds`. Documentation is filled in on `completionItem/resolve`. |
| **Signature help** | Argument lists of `MAX`, `MIN`, `ABS`, `AND`, `OR`. |
| **Inlay hints** | Names generated for unnamed constraints (`C1:`) and objectives, the `_rng` partner of ranged constraints, a variable's type after its first use, and the folded RHS when constants sit on the left. Each can be switched off. |
| **Code actions** | Quick fixes: add/remove `/ 2` on quadratic blocks, set an indicator value to 0/1, rename a duplicate, normalise `=<`/`=>`, remove an unused declaration or a duplicate type entry. Refactors: add a bound (creating `Bounds` if needed), move a variable between type sections, name all unnamed constraints, sort a type section, turn `10 >= x` into `x <= 10`. Source action `source.organizeSections` reorders sections canonically and keeps comments. |
| **Formatting** | Document, range (snapped to whole entries) and on-type (`\n`). Keeps every comment and single blank lines. Canonical spacing, one entry per line, `=<` → `<=`, wrapping at `lineWidth`, optional operator alignment, keyword casing. Never formats a file with syntax errors. Tests check it is idempotent and that the parsed model is unchanged on every fixture. |
| **Folding / selection** | Folds for sections, block comments and runs of line comments. Selection expands term → expression → constraint → section. |
| **Semantic tokens** | Full, range and delta, from the grammar's highlight query, with `declaration`, `readonly` and `defaultLibrary` modifiers. |
| **Code lens** | "N variables" on each constraint/objective; "used in N constraints" on each variable's definition (runs the client command `lp.showReferences`). |
| **Commands** | `lp.analyze` (markdown analysis report), `lp.convertToMps` (writes `<name>.mps` next to the file), `lp.showModelStats`. The first argument is the document URI. |

Positions are negotiated as UTF-8 when the client offers it, else UTF-16. Documents sync incrementally: each edit is applied to the tree-sitter tree and reparsed incrementally. The full `lp_parser_rs` parse and analysis runs debounced on a blocking thread, and stale results are dropped.

## Install

```bash
# from a checkout of this repository
cargo install --path lsp

lp-lsp --version
```

The binary speaks LSP over stdio. It needs no arguments.

## Configuration

Settings live under the `lp` section (`workspace/configuration`, refreshed on
`workspace/didChangeConfiguration`). They can also be passed as `initializationOptions`. Every field is optional.

| Setting | Default | Description |
| --- | --- | --- |
| `lp.analysis.enabled` | `true` | Publish `lp_parser_rs` analysis issues as diagnostics. |
| `lp.analysis.largeCoefficientThreshold` | `1e9` | Flag coefficients above this magnitude. |
| `lp.analysis.smallCoefficientThreshold` | `1e-9` | Flag non-zero coefficients below this magnitude. |
| `lp.analysis.largeRhsThreshold` | `1e9` | Flag right-hand sides above this magnitude. |
| `lp.analysis.coefficientRatioThreshold` | `1e6` | Flag a max/min coefficient ratio above this. |
| `lp.semantic.debounceMs` | `300` | Delay after the last edit before the semantic pass runs. |
| `lp.semantic.maxFileSizeMb` | `20` | Above this size, the semantic pass only runs on open and save. |
| `lp.format.indent` | `2` | Body indent in spaces (0–16). |
| `lp.format.lineWidth` | `100` | Wrap long expressions at this width (≥ 20). |
| `lp.format.alignOperators` | `false` | Align comparison operators within a section. |
| `lp.format.keywordCase` | `"preserve"` | `preserve`, `lower`, `upper` or `title`. |
| `lp.inlayHints.generatedNames` | `true` | Names generated for unnamed constraints and objectives. |
| `lp.inlayHints.rangePartners` | `true` | The `_rng` partner of a ranged constraint. |
| `lp.inlayHints.variableTypes` | `true` | A variable's type after its first use. |
| `lp.inlayHints.normalisedRhs` | `true` | The folded right-hand side when constants appear on the left. |

Invalid settings are rejected as a whole, with a message in the editor. The previous configuration stays active.

## Editor setup

### Neovim (0.11+)

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

The "used in N constraints" code lens runs the client-side command `lp.showReferences` (arguments: URI,
position, locations). Map it to the quickfix list:

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

### Helix

```toml
# ~/.config/helix/languages.toml
[language-server.lp-lsp]
command = "lp-lsp"

[language-server.lp-lsp.config.lp.format]
indent = 2

[[language]]
name = "lp"
scope = "source.lp"
file-types = ["lp"]
comment-token = "\\"
block-comment-tokens = { start = "\\*", end = "*\\" }
roots = []
language-servers = ["lp-lsp"]
auto-format = true
```

### Zed

Zed needs an extension to register a language, so the simplest route is a dev extension. It contains
`extension.toml` and `languages/lp/config.toml` (`name = "LP"`, `path_suffixes = ["lp"]`,
`line_comments = ["\\ "]`). Then point the language server at the binary in `settings.json`:

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

### VS Code

The extension lives in [`editors/vscode`](editors/vscode):

```bash
cd lsp/editors/vscode
npm ci && npm run package
code --install-extension lp-lsp-*.vsix
```

It starts `lp-lsp` from `PATH` (override with `lp.server.path`), contributes the settings above, and provides
a TextMate grammar so files are highlighted before the server starts.

## Performance

`cargo bench -p lp-lsp --bench lsp` on a generated 52 MB LP file (Apple silicon, 14 cores):

| Operation | Median | Before optimisation |
| --- | --- | --- |
| Single-character edit (`apply_changes`: text, line index, incremental reparse) | 246 ms | 1.44 s |
| Incremental reparse only | 228 ms | 224 ms |
| Symbol index build (8 threads above 4 MB) | 377 ms | 1.17 s |
| Semantic tokens, full document (8 threads above 4 MB) | 113 ms | 2.57 s |

How it stays responsive:

- **Keystrokes only reparse.** The symbol index is built lazily, once per version, by the first request that needs it. Documents share it when cloned.
- **Heavy work stays off the async runtime.** Requests run on the blocking pool against a snapshot of the document. Edits are applied in order, under `block_in_place`.
- **Large files (> 1 MB) debounce diagnostics.** Syntax diagnostics wait for typing to pause rather than running per keystroke. Above `lp.semantic.maxFileSizeMb`, the full semantic parse runs only on open and save.
- **Index and tokens are single walks.** Each is one tree-sitter cursor walk dispatching on symbol ids. On large files they are split into byte windows across threads and merged in order; tests check the result is identical to a single-threaded run. Semantic tokens follow `highlights.scm` (a test compares them with the query on every fixture) without the query engine.

What remains in an edit is tree-sitter's own incremental reparse: rebalancing the repetition that holds a section's entries, which grows with the section's size. Typical models (a few MB) take a few milliseconds per edit.

## Development

```bash
cargo test -p lp-lsp                       # unit, property, snapshot and end-to-end tests
cargo bench -p lp-lsp --bench lsp          # criterion benchmarks on a generated ~50 MB file
cargo build --release -p lp-lsp && python3 lsp/scripts/smoke.py target/release/lp-lsp
```

`PLAN.md` describes the architecture. The tree-sitter grammar comes from the `main` branch of
[dandxy89/treesitter-lp](https://github.com/dandxy89/treesitter-lp) (generated C sources, so building needs a C
compiler). `queries/folds.scm` is a copy of the grammar's folds query.
