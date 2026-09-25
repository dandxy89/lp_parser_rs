# LP Language Support

VS Code client for [`lp-lsp`](https://github.com/dandxy89/lp_parser_rs/tree/main/lsp), a language server for LP (linear programming) files in the CPLEX LP format with Gurobi extensions.

## Requirements

Install the server binary and make sure it is on your `PATH` (or set `lp.server.path`):

```sh
cargo install --path lsp   # from a checkout of lp_parser_rs
```

## Features

- Syntax highlighting, comment toggling and bracket matching
- Diagnostics (syntax errors and model analysis), hover, completion, signature help
- Go to definition, references, rename, document and workspace symbols
- Formatting, folding, selection ranges, semantic tokens, inlay hints, code lenses and quick fixes
- Commands (palette category **LP**): *Analyse Model*, *Convert to MPS*, *Show Model Statistics*, *Restart Language Server*

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `lp.server.path` | `lp-lsp` | Server binary; a bare name is resolved from `PATH` |
| `lp.trace.server` | `off` | Trace client/server messages |
| `lp.analysis.*` | | Toggle analysis diagnostics and their thresholds |
| `lp.semantic.maxFileSizeMb` / `debounceMs` | `20` / `300` | Full-parse scheduling |
| `lp.format.*` | | `indent`, `lineWidth`, `alignOperators`, `keywordCase` |
| `lp.inlayHints.*` | `true` | `generatedNames`, `rangePartners`, `variableTypes`, `normalisedRhs` |

## Development

```sh
npm ci
npm run compile   # or: npm run watch
npm run package   # produces lp-lsp-<version>.vsix
code --install-extension lp-lsp-*.vsix
```
