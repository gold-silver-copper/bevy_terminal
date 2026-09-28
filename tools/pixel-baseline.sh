#!/usr/bin/env bash
# Writes byte-exact renders of the current checkout to <output dir>, for
# comparing two revisions with tools/pixel-compare.py:
#
#   scenes/   every scene of fixed inputs as the main world hands it over
#             (instances, batches, atlas uploads) and the CPU replay's
#             canvas after it (crates/bevy_terminal/src/render/batch/baseline.rs);
#   fidelity/ glyph_fidelity's GPU renders in its five configurations;
#   coverage/ glyph_coverage's GPU renders;
#   exports/  the render_test and *_export examples' GPU exports.
#
# Run it from the root of each checkout, e.g. a worktree of the base:
#
#   git worktree add /tmp/base main
#   (cd /tmp/base && "$OLDPWD/tools/pixel-baseline.sh" /tmp/pixels-base)
#   tools/pixel-baseline.sh /tmp/pixels-head
#   tools/pixel-compare.py /tmp/pixels-base /tmp/pixels-head
#
# A checkout without the scene dump test gets the version first committed
# (its format is stable), since the renderer internals it reads were the
# same then. Compare on one machine and GPU backend.
set -euo pipefail
out=$1
mkdir -p "$out"
out=$(cd "$out" && pwd)
cd "$(git rev-parse --show-toplevel)"
cargo=(cargo +stable)

dump=crates/bevy_terminal/src/render/batch/baseline.rs
if [ ! -f "$dump" ]; then
    first=$(git log --diff-filter=A --format=%H --all -- "$dump" | tail -1)
    [ -n "$first" ] || { echo "no committed $dump to add" >&2; exit 1; }
    git show "$first:$dump" > "$dump"
    printf '\n#[cfg(test)]\nmod baseline;\n' >> crates/bevy_terminal/src/render/batch.rs
    echo "added the scene dump test from ${first:0:7}"
fi
rm -rf "$out/scenes"
BEVY_TERMINAL_BASELINE="$out/scenes" "${cargo[@]}" test -p bevy_terminal --all-features --lib --locked \
    pixel_baseline_dump -- --ignored

examples=(glyph_fidelity glyph_coverage render_test image_export high_dpi_export
    multiple_terminals_export ratatui_examples_export)
"${cargo[@]}" build --profile release-fast --all-features --locked "${examples[@]/#/--example=}"
run() { "${cargo[@]}" run --profile release-fast --all-features --locked --example "$@"; }

rm -rf "$out/fidelity" "$out/coverage" "$out/exports"
configs=(
    "fit|"
    "natural|--from-font 23"
    "compact|--from-font 23 --line-height 0.85"
    "fixed|--fixed-cell 9x18 --font-size 18"
    "roomy|--fixed-cell 14x24 --font-size 16"
)
for config in "${configs[@]}"; do
    name=${config%%|*}
    # shellcheck disable=SC2086
    run glyph_fidelity -- --check --font all --scale all ${config#*|} \
        --output "$out/fidelity/$name" > "$out/fidelity-$name.log"
done
run glyph_coverage -- --output "$out/coverage" > "$out/coverage.log" 2>&1

for font in 0 1 2 3 4 5; do
    rm -rf target/render-test
    run render_test -- --export --font "$font" > /dev/null 2>&1
    mkdir -p "$out/exports/render-test-$font"
    cp -R target/render-test/. "$out/exports/render-test-$font/"
done
for pair in image_export:render-qa high_dpi_export:render-qa-2x \
    multiple_terminals_export:multiple-terminals-qa ratatui_examples_export:ratatui-examples; do
    example=${pair%%:*} dir=${pair#*:}
    rm -rf "target/$dir"
    run "$example" > /dev/null 2>&1
    cp -R "target/$dir" "$out/exports/$dir"
done
# Only images and scene dumps are compared; logs and reports stay for reference.
find "$out" -type f \( -name '*.png' -o -name '*.rgba' -o -name '*.scenes' \) | wc -l | xargs echo "files written:"
