# VHS tape for the lp_diff TUI demo.
#
# Two scenes, one story: explore a diff, then interrogate a model.
#   1. Diff an MPS file against an LP file (mixed formats) and solve both.
#   2. Inspect a single model, rewrite it, and diagnose why it solves as it does.
#
# Scene 2 uses boeing2.lp deliberately: it is large enough that presolve finds
# real reductions and the diagnostics tables have something to rank. The fit2d
# model in scene 1 presolves to no change, so it would film an empty result.
#
# Render:   vhs tui/scripts/demo.tape          (from the repo root)
# Optimise: gifsicle -O3 --batch tui/assets/demo.gif

Output tui/assets/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1280
Set Height 800
Set Theme "Catppuccin Mocha"
Set CursorBlink false
Set TypingSpeed 35ms

# --- setup (off camera) ---
Hide
Type "cd /Users/dan/Projects/lp_parser_rs && export PATH=$PWD/target/release:$PATH"
Enter
Type "clear"
Enter
Show

# ===========================================================================
# Scene 1 — diff two models across formats
# ===========================================================================

Type "lp_diff rust/resources/mps/fit2d.mps rust/resources/fit1d.lp --theme dark"
Sleep 600ms
Enter
Sleep 2.5s

# Constraints: coefficient-level side-by-side diff
Type "3"
Sleep 2s
Type "j"
Sleep 900ms
Type "j"
Sleep 1.5s

# Raw text view: the actual file lines, side by side
Type "r"
Sleep 2.2s
Type "r"
Sleep 1s

# Live tolerance cycling: rebuilds the diff on the fly
Type "t"
Sleep 1.8s

# Telescope-style fuzzy search across all sections
Type "/"
Sleep 700ms
Type "R0100003"
Sleep 1.6s
Enter
Sleep 1.4s

# Numerics: per-file numerical conditioning
Type "5"
Sleep 2s

# HiGHS solve: solve BOTH files and diff the solutions
Type "S"
Sleep 1.2s
Type "3"
Sleep 3.5s

# Variables tab, then changed-rows-only
Type "2"
Sleep 1.8s
Type "d"
Sleep 1.8s
Escape
Sleep 800ms

# ===========================================================================
# Scene 2 — inspect one model, rewrite it, diagnose it
# ===========================================================================

# Swap files off camera so the demo never cuts to a bare shell prompt.
Hide
Type "q"
Sleep 400ms
Type "clear"
Enter
Show

Type "lp_diff rust/resources/boeing2.lp --theme dark"
Sleep 600ms
Enter
Sleep 2.5s

# Rewrite: the presolve rule picker, then original vs rewritten
Type "P"
Sleep 2.5s
Enter
Sleep 3.5s

# The comparison header carries what the rewrite achieved
Sleep 1.5s
Escape
Sleep 800ms

# Diagnostics: which rows and variables are behind the solve behaviour
Type "D"
Sleep 3.2s
Type "j"
Sleep 400ms
Type "j"
Sleep 2.8s

# End on the diagnostics tables; quit off camera.
Hide
Escape
Type "q"
