#!/usr/bin/env bash
# Render every scene and optimise the GIFs losslessly. Run from the repo root.
set -euo pipefail
cd "$(dirname "$0")/../../.."
for t in docs/showcase/tapes/*.tape; do [[ $t == */setup.tape ]] && continue; (cd docs/showcase/tapes && vhs "$(basename "$t")"); done
gifsicle -O3 --batch docs/showcase/assets/*.gif
