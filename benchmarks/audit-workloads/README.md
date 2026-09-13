# Audit CPU and allocation workloads

This standalone executable measures the retained surface and main-world text
pipeline. It creates no window or GPU renderer. It complements the renderer
comparison suite with library-specific lifecycle and cache workloads:

- Scrolling equal ASCII, equal heap-backed graphemes, and changing heap-backed graphemes.
- Thirty-two idle terminals with sixteen registered font assets.
- Repeated grid, font, and raster-scale changes.
- A Unicode working set exceeding the shape-cache entry limit, followed by changing ASCII frames.

Run from the repository root:

```sh
CARGO_TARGET_DIR="$PWD/target" cargo run --manifest-path benchmarks/audit-workloads/Cargo.toml --locked
```

Stdout is TSV: elapsed nanoseconds, allocation/reallocation counts, requested
allocation bytes, peak additional live requested bytes, and shape misses.
Stderr also reports retained CPU image bytes for the idle-terminal setup.
The executable forwards allocations to Rust's `System` allocator and counts
them with atomics. This small unsafe instrumentation is confined to this
unpublished benchmark workspace; production crates remain safe Rust.

Use the same source, lockfile, feature set, toolchain, profile, and hardware
for baseline and changed checkouts. Copy this directory into the baseline
checkout so its relative dependency selects that checkout's core library.
Build first, then run each binary repeatedly without competing builds or GPU
workloads. Compare medians and retain raw output. Debug-profile timings are
useful for comparing these code paths, not production frame-rate predictions.

Allocation counts cover the entire process, including Bevy worker threads.
They exclude allocator headers and fragmentation. Reallocation accounts for
the new requested size; peak live bytes measure requested allocations, not
RSS or any temporary storage internal to the allocator. Idle measurement
excludes initialization; the separate CPU-image total includes retained
atlas storage after initialization. Churn and saturation measurements include
producer writes as well as renderer synchronization and shaping.
