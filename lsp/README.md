# lp-lsp

A [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) implementation for
LP (linear programming) files: the CPLEX LP format plus the Gurobi extensions accepted by
[`lp_parser_rs`](../rust) (multi-objectives, general constraints, indicators, lazy constraints, user cuts,
quadratic terms, semi-continuous variables, SOS sets).

Syntax is handled by an incremental, error-tolerant [tree-sitter](https://tree-sitter.github.io/) grammar
(`tree-sitter-lp`), so every keystroke gets fresh diagnostics, symbols and highlighting even in broken files.
Semantics come from a debounced full `lp_parser_rs` parse and analysis, run off the main thread.

<!-- FEATURES -->

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

<!-- BENCHMARKS -->

## Development

```bash
cargo test -p lp-lsp                       # unit, property, snapshot and end-to-end tests
cargo bench -p lp-lsp --bench lsp          # criterion benchmarks on a generated ~50 MB file
cargo build --release -p lp-lsp && python3 lsp/scripts/smoke.py target/release/lp-lsp
```

`PLAN.md` describes the architecture. The tree-sitter grammar comes from the `main` branch of
[dandxy89/treesitter-lp](https://github.com/dandxy89/treesitter-lp) (generated C sources, so building needs a C
compiler). `queries/folds.scm` is a copy of the grammar's folds query.
