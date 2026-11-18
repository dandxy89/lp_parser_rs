# Showcase deck

A self-contained HTML presentation on why LP files deserve a grammar, and what `lp_parser_rs` builds on one.

Published on the docs site at <https://dandxy89.github.io/lp_parser_rs/showcase/>. Locally, open `index.html` in a browser (no build step, no dependencies beyond Google Fonts).

| Key | Action |
| --- | --- |
| `→` / `Space` / `←` | Next / previous slide |
| `S` | Speaker notes |
| `O` / `Esc` | Overview grid |
| `F` | Fullscreen |
| `1`–`5` | Switch CLI demo tab (slide 7) |

Interactive slides: hover the annotated LP file (3), click the architecture boxes (5), switch CLI demos (7), and edit the model in the playground (12). Slide 11 covers the VS Code extension; slide 15 lists limits and a proposed pilot.

## Regenerating the demos

`demo/` holds the sample models; `tapes/` holds the VHS scenes. With `vhs`, `gifsicle`, `lp_parser` (built with `--features cli,csv,diff,serde,lp-solvers`), `lp-lsp` and `nvim` on `PATH`:

```bash
docs/showcase/tapes/render.sh
```

`assets/tui.gif` is a copy of `tui/assets/demo.gif`; `assets/vscode.png` is a copy of `lsp/editors/vscode/images/screenshot.png`.
