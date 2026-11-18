# Shared settings, sourced by every scene. Render all: ./render.sh
# Puts this workspace's release build first on PATH, so build it first:
#   cargo build --release -p lp_parser_rs --features cli,csv,diff,serde,lp-solvers
# lp-lsp and nvim come from your PATH.
Set Shell "bash"
Set FontSize 20
Set Width 1400
Set Height 760
Set Padding 28
Set Theme "Catppuccin Mocha"
Set CursorBlink false
Set TypingSpeed 40ms
Hide
Type `cd ../demo && export PATH="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/release:$PATH" PS1='\[\e[35m\]❯\[\e[0m\] ' && clear`
Enter
Show
