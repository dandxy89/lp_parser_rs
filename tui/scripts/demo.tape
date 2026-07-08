# VHS tape for the lp_diff TUI demo — a tour of the main features.
# Render:  vhs tui/scripts/demo.tape   (run from the repo root)
# Then optimise the GIF losslessly:  gifsicle -O3 --batch tui/assets/demo.gif

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

# --- launch: diff an MPS file against an LP file (mixed formats, opens on Summary) ---
Type "lp_diff rust/resources/mps/fit2d.mps rust/resources/fit1d.lp --theme dark"
Sleep 600ms
Enter
Sleep 2.5s

# --- Constraints: coefficient-level side-by-side diff ---
Type "3"
Sleep 2s
Type "j"
Sleep 1s
Type "j"
Sleep 1.5s

# --- raw text view: the actual file lines, side by side ---
Type "r"
Sleep 2.2s
Type "r"
Sleep 1s

# --- filters: modified-only, then back to all ---
Type "m"
Sleep 1.5s
Type "a"
Sleep 1s

# --- sort cycling: name -> |delta| ---
Type "s"
Sleep 1.5s

# --- live tolerance cycling: rebuilds the diff on the fly ---
Type "t"
Sleep 1.8s

# --- telescope-style fuzzy search across all sections ---
Type "/"
Sleep 800ms
Type "R0100003"
Sleep 1.8s
Enter
Sleep 1.5s

# --- Numerics: per-file numerical conditioning ---
Type "5"
Sleep 2s

# --- help pop-up: every key binding in one place ---
Type "?"
Sleep 2.5s
Escape
Sleep 600ms

# --- HiGHS solve: solve BOTH files and diff the solutions ---
Type "S"
Sleep 1.2s
Type "3"
Sleep 3.5s

# browse the result tabs: Variables, then diff-only + threshold cycling
Type "2"
Sleep 2s
Type "d"
Sleep 1.8s
Type "t"
Sleep 1.8s

# Log tab, then back to Summary
Type "4"
Sleep 1.6s
Type "1"
Sleep 2s

# --- quit off camera so the demo ends on the solver results ---
Hide
Escape
Type "q"
