# Low-level renderer cleanup

Status: implementation and local verification complete. Changes are uncommitted.

## Resulting boundary

`bevy_terminal` turns a shared cell surface and rendering configuration into an
image, validated geometry, and status. It owns neither presentation nor window/DPI
policy. `bevy_terminal_ratatui` supplies a raw Ratatui backend and an optional
terminal wrapper.

Removed:

- `TerminalWorldQuad`, its mesh/material synchronization, and both `3d` features.
- Core UI mutation, both `ui` features, presentation detection, automatic window
  scaling, and direct window/core-pipeline dependencies.
- `TerminalRenderScale`; `RasterConfig.scale` is now an explicit `f32`, default 1.
- `TerminalReady`, `TerminalRemeasured`, and readiness-history bookkeeping.
- `TerminalFonts`, the `SmolStr` re-export/direct dependency, and window helpers.
- The adapter's wildcard renderer re-export. Its curated prelude and explicit
  `bevy_terminal` re-export provide the supported entry points.
- The redundant imported-screen example and duplicate core interactive UI demo.

Kept: transactional surfaces, dirty tracking, wide-cell occupancy, styles, font
measurement/fallback, all three sizing modes, validated geometry, stable image
handles, bounded caches, and both Ratatui integration styles.

CPU timing instrumentation is now opt-in (`timings`); work counters remain always
available. Timing fields are zero when disabled. This removes clock reads from
normal consumers without removing diagnostic counters; benchmarks opt in.

`RatatuiTerminal::fit_to` additionally rejects geometry from another surface. Its
existing fitting regression now exercises foreign as well as stale geometry.

## Examples and consumers

- Shared adapter example code owns UI layout and, separately, optional window-DPI
  policy. Fixed-scale exports do not enable window scaling.
- `world_quad` owns an ordinary Bevy mesh, material, and transform, and binds the
  output image after measurement. It requires no library presentation feature.
- `scene_export` writes neutral cells directly and waits for a correctly sized,
  nonzero-alpha GPU readback of its static scenes. This is not advertised as an
  arbitrary animated-frame completion protocol.
- Export and fidelity harnesses use persistent measurement queries with their own
  per-export bookkeeping, rather than renderer readiness events.
- The real `bevy_ratatui` fixture owns its image binding and retains
  `ratatui::Terminal<RatatuiBackend>` as the context's dereference target.
- Ratty changes are isolated in `../ratty-low-level`, branch
  `refactor/terminal-low-level`, based on `9815dd6`. Its original worktrees and the
  sibling `bevy_ratatui` checkout are untouched. Ratty owns the framebuffer-ratio
  helper and its headless snapshot tool adopts persistent output.

No commits, pushes, or GitHub mutations are part of this cleanup. Ratty verification
uses `/tmp/ratty-low-level/patch.toml` to select these unpublished local libraries:

```toml
[patch."https://github.com/gold-silver-copper/bevy_terminal"]
bevy_terminal_ratatui = { path = "/Users/kisaczka/Desktop/code/bevy_grid" }
bevy_terminal = { path = "/Users/kisaczka/Desktop/code/bevy_grid/crates/bevy_terminal" }
```

Before publishing the consumer migration, publish/commit the library and replace
Ratty's Git pin with that revision. This report does not claim that publication
has happened.

## Verification

All workspace commands passed:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --lib --tests --locked
cargo test --workspace --all-features --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
WGPU_BACKEND=metal cargo test --workspace --all-features --test gpu_readback --test export_readiness --locked -- --ignored --test-threads=1
```

Results: 82 core tests, 18 adapter tests, 10 doctests, and six explicit GPU/readback
regressions passed. Formatting and diff whitespace checks also passed in Ratty.

A standalone application depending only on `bevy_terminal` with defaults disabled
rendered supplied-font cells and verified a correctly sized, nonzero-alpha Metal
readback. Core default, core minimal, and adapter minimal dependency trees exclude
`bevy_ui`, `bevy_ui_render`, `bevy_pbr`, `bevy_core_pipeline`, and `bevy_winit`.
Bevy still brings `bevy_window` transitively through rendering; this is not a claim
that every window-related crate disappears.

The `scene_export`, `image_export`, and `high_dpi_export` runs passed. Exported
neutral scenes, the resized camera capture, and the 2× texture were inspected.
The fixed-cell fidelity command (`--check --font all --scale all`) still exits 1:
66 of 100 groups fail, with group results and every glyph diagnostic identical
to the preceding verified baseline. This is a pre-existing limitation, not a green
check. The font-driven tile command (adding `--from-font 23.0 --tiles-only`) passes
all 24 groups, also identical to baseline. The cleanup does not change glyph
fitting algorithms or claim to resolve those existing clipping/seam failures.

Ratty passed all-target/all-feature checking and strict Clippy, 211 tests and one
doctest; three existing VT tests remain ignored. Its rebuilt snapshot executable
passed all seven Metal smoke cases with zero stale cells, including inline images,
compact line height, fallback fonts, child exit, styles, and scrolling. Tests cover
explicit window-scale forwarding and retained layout during unavailable output.
The final snapshot smoke run includes the change-detection fix described below.

The actual `bevy_ratatui` context test passes, as does the fixture's
`consumer-windowed` build. These checks use the real consumer trait and exercise
raw terminal drawing, geometry adoption, resizing, and application image binding.
The native-only consumer check follows the repository workflow. Full consumer
migration, input handling, and native terminal lifecycle changes are outside this
prompt's scope.

Both standalone benchmark packages pass checking and strict Clippy. Their timing
feature is explicit. The audit workload lockfile removes 171 unused packages;
no packages were added or upgraded in any changed library/fixture/benchmark
lockfile. Runtime performance comparisons were not rerun; no speedup is claimed.

## Completion audit and review

The final review covered the full working diff against library HEAD `409b7f2`,
Ratty's two source changes against `9815dd6`, lockfile package changes, deleted
presentation/example code, and new prompt/report/ignore files. There are no staged
changes. This was a separate full-diff review pass by the implementing agent;
no independent reviewer was used for this non-PR task.

| Prompt requirement | Implementation and evidence |
| --- | --- |
| 1. Remove library 3D presentation | Deleted core module and both feature declarations; owned mesh/material example compiles in all-target check. |
| 2. Separate UI presentation | Core has no presentation query or mutation; example helpers and context fixture own binding; core regression tests require UI to remain untouched. |
| 3. Explicit raster scale | Numeric configuration, bounded sanitization, no core window/UI lookup; fractional geometry tests and Ratty framebuffer-ratio test pass. |
| 4. Persistent readiness | Events/history removed; consumers query after Sync; tests prove late access, stale rejection, stable image handles, logical-only geometry changes, and pending/failure behavior. GPU tests separately verify rendering. |
| 5. Reduce conveniences | Font wrapper and window helpers removed; direct FontCx documentation compiles; curated exports and optional timings compile in minimal and all-feature configurations. |
| Preserve low-level behavior | Existing surface, dirty tracking, Unicode/style/cursor, font, sizing, cache, image-handle and GPU regressions pass; fidelity diagnostics remain identical. |
| Preserve consumer choices | Raw backend and wrapper remain; real context fixture and isolated Ratty migration pass their checks. |
| Update supporting code | Documentation/doctests, examples, test harnesses, benchmark packages and migration fixture updated and verified. |
| No external publication | Library and isolated Ratty changes remain uncommitted; no pushes or GitHub mutations. |

Review fixes included rejecting foreign-surface geometry in `fit_to`, replacing two
obsolete UI-mutation test expectations with application-ownership assertions, and
preventing Ratty's persistent snapshot poll from marking its resource changed or
resetting its capture gate on unchanged output. Affected checks were rerun after
these fixes. No confirmed in-scope findings remain.

Ratty's tracked lockfile retains the published Git revision. Its tested local
resolution is saved at `/tmp/ratty-low-level/Cargo.local.lock`. For local work, run
Cargo with `--config /tmp/ratty-low-level/patch.toml` once without `--locked` to
resolve the unpublished libraries, then use `--locked` for subsequent checks.
Publication must update the Git pin to the actual future library commit.

Verification ran locally on macOS/Metal. No new Linux/Vulkan or Windows CI result
is claimed for this unpublished diff. The application-owned 3D example was compiled;
it was not interactively exercised. Ratty's application scene smoke captures did
exercise its own presentation path.

Machine-readable results, requirement evidence, and source hashes are recorded in
`CODEBASE_IMPLEMENTATION_LOW_LEVEL_VERIFICATION.json`. Detailed logs and images
remain under `/tmp/terminal-low-level-verify`, `/tmp/ratty-low-level`, repository
`target` directories, and `/tmp/terminal-low-level-*.log`. Temporary artifacts are
local evidence, not committed test fixtures. Prior audit reports and prompts remain
historical records of their own API versions.
