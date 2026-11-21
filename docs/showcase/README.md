# Showcase deck

A self-contained HTML presentation on why LP files deserve a grammar, and what `lp_parser_rs` builds on one.

Published on the docs site at <https://dandxy89.github.io/lp_parser_rs/showcase/>. Locally, open `index.html` in a browser (no build step, no dependencies beyond Google Fonts).

| Key | Action |
| --- | --- |
| `→` / `←` | Next / previous topic |
| `↓` / `↑` | Next / previous slide within a topic (the dots on the right show where you are) |
| `Space` / `PageDown` / `PageUp` | Step through every slide in order, sub-slides included |
| `S` | Speaker notes |
| `O` / `Esc` | Overview grid |
| `F` | Fullscreen |
| `1`–`5` | Switch CLI demo tab (CLI topic) |

Links take the form `#8` (topic 8) or `#8.2` (its second sub-slide). Topics: CLI (every command, CI), the `lp_diff` TUI (diffing, solving, rewrite, analysis panes), the language server (diagnostics, navigation, any editor, `lp/*` requests), the VS Code extension (explorer, model views, solve and explain, extras), `lp-highs` (and its protocol), the playground, Python (diagnostics), use cases, limits and install.

Interactive slides: hover the annotated LP file, click the architecture boxes, switch CLI demos, and edit the model in the playground.

## Regenerating the demos

`demo/` holds the sample models; `tapes/` holds the VHS scenes. With `vhs`, `gifsicle`, `lp_parser` (built with `--features cli,csv,diff,serde,lp-solvers`), `lp-lsp` and `nvim` on `PATH`:

```bash
docs/showcase/tapes/render.sh
```

`assets/tui.gif` is a copy of `tui/assets/demo.gif`; `assets/vscode.png` is a copy of `lsp/editors/vscode/images/screenshot.png`.
