# lp-highs

[HiGHS](https://highs.dev) solver integration for LP and MPS models parsed by
[`lp_parser_rs`](https://crates.io/crates/lp_parser_rs).

- `lp_highs::solve`: build a HiGHS model from an `LpProblem`, solve it (with
  cancellation and `highs.opt` support), and diagnose infeasibility by elastic
  relaxation.
- `lp_highs::query`: unbounded rays, irreducible infeasible subsystems (IIS)
  and ranging.
- `lp_highs::protocol`: the JSON protocol spoken by the `lp-highs` helper binary.
- `lp_highs::run_request`: one protocol request, end to end.

## Features

| Feature | Default | Effect |
|---|---|---|
| `highs` | yes | Links HiGHS (built from source: needs cmake, a C++ compiler and libclang). Enables `solve`, `query`, `run_request` and the `lp-highs` binary. |

Depend on the crate with `default-features = false` to get the `protocol`
module alone, with no C++ toolchain and no `lp_parser_rs` in the tree:

```toml
lp-highs = { version = "0.1", default-features = false }
```

## The `lp-highs` helper

```text
lp-highs info        one JSON object: name, version, protocol, HiGHS version
lp-highs --version   lp-highs <version>
lp-highs solve       one JSON request on stdin, one JSON response on stdout
```

Progress is streamed on stderr as one JSON object per line
(`{"event":"phase","phase":"building","elapsedMs":12}`; phases are `parsing`,
`building`, `solving`, `extracting`, `diagnosing`). The helper exits 0 whenever
it wrote a response (infeasible models included), 2 for usage and protocol
errors, and 1 for an internal failure. A `highs.opt` file in the working
directory applies to the solve. Killing the process cancels the solve entirely.

Request:

```json
{ "protocol": 1, "format": "lp", "text": "min\nobj: x\nst\nc1: x >= 1\nend",
  "options": { "timeLimit": 60, "mipRelGap": 0.01, "presolve": "choose", "threads": 4 },
  "diagnose": "auto", "includeLog": false }
```

See `tests/fixtures/protocol/` for complete request and response examples.

## Install

```sh
cargo install lp-highs
```

## Licence

MIT OR Apache-2.0
