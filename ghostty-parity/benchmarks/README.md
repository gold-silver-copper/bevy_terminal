# Benchmark results

- Machine: Apple M2 Max (64 GiB), macOS, Metal, Rust 1.98.1 stable.
- Profiles: each benchmark workspace's own (`renderer-comparison`: thin LTO, one codegen unit; `audit-workloads`: stock release).
- Variants, each built in its own worktree and run interleaved, rotating order:
  - `main`: `9281c92`, the released 0.7.7 source;
  - `pre`: `32e65c7`, after overflow, constraints, sprites and blending, before the performance work;
  - `post`: the performance commit that follows these files in the PR.

**Caveat.** The machine was heavily loaded by unrelated work throughout: other compilers, a game test suite and a browser, with a load average of 16–60 on 12 cores. Wall-clock medians moved by up to 2× between identical runs. For that reason the files also report low percentiles (`p10`) over every frame and the *fastest* of many updates, which other load can only inflate, never deflate. Compare variants within a file, not across files.

| File | What `post` contained | Load average |
| --- | --- | --- |
| `renderer-batch1-medians.txt` | render-world atlas and shape-cache generations, *before* the background-run and scene fast-path fixes | 20–50 |
| `renderer-batch3-p10.txt` | plus the background-run binary search and single style resolution | 16–38 |
| `renderer-batch4-p10.txt` | plus opaque block elements as solid quads | 21–53 |
| `audit-release-run2.txt` | plus the emission fast path and per-cell linear foreground | 36–37 |
| `audit-release-run3.txt` | final code (linear background runs) | 21–36 |

## Reproduce

```sh
cd benchmarks/renderer-comparison
./run.sh --profile standard --adapters bevy_terminal_ratatui --repeat 1 --order-offset N --output DIR
cd ../audit-workloads
CARGO_TARGET_DIR="$PWD/../../target/audit-release" cargo build --release --locked
../../target/audit-release/release/terminal-audit-workloads
```

Copy `benchmarks/audit-workloads` into the baseline worktree first, so that every variant runs the same workloads.
