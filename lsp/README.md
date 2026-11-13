# lp-lsp

A language server for LP (linear programming) files. It understands the CPLEX LP format and the Gurobi
extensions that [`lp_parser_rs`](../rust) supports, and it keeps working while a file is half-written.

## What it does

- **Diagnostics**: as you type, syntax errors and structural checks (duplicate names, a variable in more than
  one type section, conflicting bounds, unused declarations, `=<`/`=>` spellings); after a short pause, errors
  from the full parse and analysis warnings such as badly scaled coefficients. Both push and pull diagnostics
  are supported, including workspace diagnostics for `*.lp` files that are not open.
- **Navigation**: go to definition, declaration (a variable's type-section entry) and type definition, find
  references, highlights, linked editing of a name, call hierarchy (which constraints use a variable, and which
  variables a constraint uses), and document and workspace symbols.
- **Rename**: a variable within its file, or a constraint, objective or SOS name across every `*.lp` file in the
  workspace. It refuses names that would change how the file parses.
- **Hover and completion**: details on variables, constraints and keywords, and suggestions that fit where
  the cursor is.
- **Formatting**: whole document, a selection, or on Enter. It never touches a file with syntax errors.
- **Code actions**: quick fixes for the problems it reports, plus refactors such as adding a bound, moving a
  variable between type sections and reordering sections.
- **Extras**: inlay hints, semantic highlighting, folding, selection ranges, code lens (variable counts on
  constraints and objectives, usage counts on variables) and signature help for `MAX`, `MIN`, `ABS`, `AND` and `OR`.
- **Commands**: `lp.analyze` (analysis report), `lp.convertToMps` (writes `<name>.mps` next to the file,
  replacing any existing one) and `lp.showModelStats`.

The server indexes every `*.lp` file under the workspace folders at start-up, skipping `.git`, `target`,
`node_modules`, `.venv`, `venv` and `__pycache__`, and stopping once the indexed text reaches 256 MB. It keeps the
index current through file watching or, for clients without it, file-operation notifications.

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
| `analysis.enabled` | `true` | Show analysis warnings. |
| `analysis.largeCoefficientThreshold` | `1e9` | Flag larger coefficients. |
| `analysis.smallCoefficientThreshold` | `1e-9` | Flag smaller non-zero coefficients. |
| `analysis.largeRhsThreshold` | `1e9` | Flag larger right-hand sides. |
| `analysis.coefficientRatioThreshold` | `1e6` | Flag a larger max/min coefficient ratio. |
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

## Editor setup

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

## Development

```bash
cargo test -p lp-lsp
```
