# Measured audit workloads

Baseline: `0042ac5b8fbaa3871a1cf61c659769c739123d79`. Changed: final local audit implementation, including cache reset and weak geometry source ownership. Apple M2 Max, macOS; unoptimized dev profile, allocation instrumentation enabled. Three alternating process runs of each prebuilt executable; medians below. Compiler activity from other work overlapped the first pair and second baseline run (see `sampling.json`). These shared-machine elapsed times are exploratory; allocation counts and shape misses are the stronger evidence. The last pair had no compiler processes observed before or after either run. No additional builds or GPU runs were launched by this task during sampling.

| Workload | Steps | Baseline ms | Changed ms | Baseline allocations | Changed allocations | Baseline misses | Changed misses |
|---|---:|---:|---:|---:|---:|---:|---:|
| scroll_equal_ascii | 200 | 19.15 | 12.96 | 0 | 0 | 0 | 0 |
| scroll_equal_heap | 200 | 44.55 | 15.37 | 384000 | 16000 | 0 | 0 |
| scroll_changing_heap | 200 | 69.86 | 54.10 | 750160 | 382160 | 0 | 0 |
| idle_32_fonts_16 | 200 | 143.68 | 56.23 | 6971 | 6154 | 0 | 0 |
| resize_font_scale_churn | 48 | 226.35 | 247.44 | 8342 | 8156 | 48 | 48 |
| unicode_cache_saturation | 12 | 1524.91 | 1439.00 | 94025 | 95631 | 5760 | 5760 |
| ascii_after_saturation | 20 | 1021.04 | 18.29 | 116228 | 10583 | 9600 | 16 |

Retained CPU image bytes for 32 blank terminals: **537,919,488 baseline → 1,048,704 changed**. The difference is the 32 eager 16 MiB atlases replaced by 4-byte placeholders; the remaining image data includes Bevy’s font atlas. This is CPU image data, not total process RSS or GPU memory.

The churn workload begins with the first actual glyph after a blank setup. Its changed run therefore includes allocating the lazy 16 MiB atlas, whereas the baseline paid that cost during setup. This deliberate cold-path tradeoff is visible in the allocated-byte columns of the raw TSV; it is not a steady-state churn speedup claim.

Before changing cache admission, the new implementation still produced **9,600 ASCII shape misses** after saturation (20 changing 480-cell frames). Resetting a full cache reduced that to **16**, admitting the new working set without per-hit LRU bookkeeping. Individually oversized runs bypass the cache without evicting useful entries.

Scrolling equal heap symbols drops from 384,000 to 16,000 allocation/reallocation calls across 200 scrolls; changing heap symbols avoid the additional temporary source clones as well. Idle allocation counts include Bevy’s own scheduling; the library’s separate regression test verifies zero idle font-catalog rescans.

Raw TSV and stderr logs are retained here. Elapsed values include atomic allocator instrumentation and are not predictions of production frame rates. See the parent README for scope and reproduction instructions.

The baseline harness has one redundant `&symbol` borrow in scrolling setup that the changed harness removes to satisfy Clippy. This is outside all measurement regions; measured workload code, dependencies, features, and profile are otherwise identical. Binary SHA-256 values and sampling activity are recorded in `sampling.json`.
