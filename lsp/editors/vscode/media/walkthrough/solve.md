# Solve with HiGHS

*LP: Solve Model* sends the model to the HiGHS solver and shows the status, objective value, activities and slack in a panel. Values also appear as hover text and inlay hints.

The solver is a separate helper, `lp-highs`, which must be installed:

```sh
cargo install --locked lp-highs
```

If it is installed somewhere unusual, set `lp.solver.path`.

Shortcut: `Ctrl+Alt+Enter` (`Cmd+Alt+Enter` on macOS) in an LP editor.

If the model is infeasible, try *LP: Diagnose Infeasibility*.
