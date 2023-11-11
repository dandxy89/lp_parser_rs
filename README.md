# LP File Parser

[![Cargo Test](https://github.com/dandxy89/congenial-enigma/actions/workflows/cargo_test.yml/badge.svg)](https://github.com/dandxy89/congenial-enigma/actions/workflows/cargo_test.yml)

A Rust LP file parser leveraging [PEST](https://docs.rs/pest/latest/pest/) and adhering to the following specifications:

- [IBM v22.1.1 Specification](https://www.ibm.com/docs/en/icos/22.1.1?topic=cplex-lp-file-format-algebraic-representation)
- [fico](https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html)
- [Gurobi](https://www.gurobi.com/documentation/current/refman/lp_format.html)

## Crate Supports

- Problem Name
- Problem Sense
- Objectives
  - Single-Objective Case
  - Multi-Objective Case
- Constraints
- Bounds
- Variable Type: Integer, Generals, Lower Bounded, Upper Bounded, Free & Upper and Lower Bounded

## TODOs List

- Remaining LP format changes:
  - Semi-continuous
    - <https://www.ibm.com/docs/en/icos/22.1.1?topic=representation-mip-features-in-lp-file-format#File_formats_reference.uss_reffileformatscplex.LP_MIP__File_formats_reference.uss_reffileformatscplex.177272__title__1>
    - <https://www.fico.com/fico-xpress-optimization/docs/dms2020-03/solver/optimizer/HTML/chapter10_sec_section102.html>
  - Special ordered sets: SOS
    - Keyword: `SOS`
    - Example: `Sos101: 1.2 x1 + 1.3 x2 + 1.4 x4 = S1`
  - Lazy Constraints
- Extensions:
  - Compares two LP files
  - CLI

## Test data

Test data has been taken from another similar project:

- [asbestian/jplex](https://github.com/asbestian/jplex/blob/main/instances/afiro.lp)
