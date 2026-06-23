# VHS tape for the lp_diff TUI demo.
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

# --- launch: compare two LP files ---
Type "lp_diff rust/resources/fit1d.lp rust/resources/fit2d.lp --theme dark"
Sleep 600ms
Enter
Sleep 2.5s

# --- Constraints: coefficient-level side-by-side diff (the money shot) ---
Type "3"
Sleep 2s
Type "j"
Sleep 1s
Type "j"
Sleep 1.5s

# --- Telescope-style fuzzy search across all sections ---
Type "/"
Sleep 800ms
Type "R010003"
Sleep 1.8s
Enter
Sleep 1.5s

# --- HiGHS solve: pick file 1, browse the result tabs ---
Type "S"
Sleep 1s
Type "1"
Sleep 2.8s
Type "2"
Sleep 1.6s
Type "1"
Sleep 1.5s

# --- close solver and quit off camera so the demo ends on the UI ---
Hide
Escape
Type "q"
