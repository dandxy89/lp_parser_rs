# lp-lsp plan

Language server for `.lp` files: tree-sitter (`tree-sitter-lp`) for syntax,
`lp_parser_rs` for semantics. Stdio transport via `tower-lsp-server` on tokio.

## Module layout

```
lsp/
  src/main.rs              stdio entry point (`lp-lsp`, `--version`)
  src/lib.rs               re-exports
  src/server.rs            Backend: LanguageServer impl, state, scheduling, dispatch
  src/document.rs          Document: text + Tree + LineIndex + SymbolIndex (+ semantic result)
  src/position.rs          Encoding + LineIndex: every byte <-> Position conversion
  src/syntax.rs            parser (thread-local), node kinds, node helpers, number parsing
  src/index.rs             SymbolIndex: variables/occurrences/roles, entities, sections, duplicates
  src/semantic.rs          full lp_parser_rs parse + analysis (blocking)
  src/config.rs            `lp.*` settings
  src/workspace.rs         *.lp discovery and loading
  src/features/*.rs        one module per LSP feature, pure functions over &Document
  queries/folds.scm        copy of the grammar's folds query
  benches/                 criterion benchmarks
  tests/                   end-to-end tests through the tower service
  editors/vscode/          thin VS Code client
```

## Key types and contracts

- `Document { uri, text, version, tree, lines, index, encoding, semantic_result }`.
  `doc.offset(Position)`, `doc.position(usize)`, `doc.range(Range<usize>)`,
  `doc.byte_range(lsp::Range)`, `doc.location(..)`, `doc.node(range, kind)`,
  `doc.semantic()` (current version only), `doc.last_semantic()`.
- `SymbolIndex { variables, entities, attributes, sections, duplicates }`, with
  `variable(name)`, `symbol_at(offset) -> (Range, Symbol)`, `entity_at(offset)`,
  `entities_named(name, namespace)`, `entity_variables(entity)`.
  - `Role`: ObjectiveTerm, ConstraintTerm, QuadraticTerm, Bound, Generals,
    Integers, Binaries, SemiContinuous, SosEntry, Indicator, Resultant,
    GeneralArgument.
  - `Variable::definition()` = first Bound else first occurrence;
    `Variable::declaration()` = first type-section entry.
  - Namespaces: objectives; constraints + general constraints + SOS (shared,
    as upstream stores them in one map).
- Feature functions take `&Document` (+ `Position`/settings) and return LSP
  types. The server owns scheduling, locking and client I/O.
- Diagnostic codes live in `features::diagnostics::codes`; code actions match
  on them.
- Keyword rule (mirrors the external scanner / upstream lexer): single-word
  section keywords (`bounds`, `generals`, `gen`, `integers`, `binaries`, `bin`,
  `semi`, `semis`, `semi-continuous`, `sos`, `end`, `genconstrs`) are keywords
  only as the first token of a line and not followed by `:`. Rename rejects
  names that would be read as one; completion only offers them at line start.

## Scheduling

- Syntax + index: synchronous on every change (incremental reparse).
- Semantic pass: debounced (`lp.semantic.debounceMs`, 300 ms) and on
  open/save, on `spawn_blocking`; generation counter drops superseded runs,
  version check drops stale results. Above `lp.semantic.maxFileSizeMb`
  (20 MB) it runs only on save.
- Diagnostics: pull when the client supports `textDocument/diagnostic`
  (refresh after each semantic pass), else push.

## Phases

1. Core: crate, position encoding, document store + incremental sync, syntax
   helpers, symbol index, config, workspace scan, server wiring. (done first,
   sequentially: every feature depends on it)
2. In parallel, one agent per group:
   - diagnostics (+ `AnalysisIssue` location in `lp_parser_rs`)
   - symbols, navigation, code lens, folding, selection
   - rename
   - hover, completion, signature help
   - inlay hints, commands
   - code actions
   - formatter
   - semantic tokens, end-to-end tests, smoke test, benchmark
   - VS Code client
3. Integration: merge, workspace build/clippy/test, README, changelog,
   Neovim config, smoke test against the release binary.

## Grammar dependency

`tree-sitter-lp` is a git dependency on the `main` branch of
https://github.com/dandxy89/treesitter-lp (`Cargo.lock` pins the resolved
commit). The crate exports `HIGHLIGHTS_QUERY` and `LOCALS_QUERY` but not the
folds query, so `queries/folds.scm` is a copy; keep it in sync with the grammar.
