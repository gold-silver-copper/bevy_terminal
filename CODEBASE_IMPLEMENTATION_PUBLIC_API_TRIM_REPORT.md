# Public API trim implementation

Implemented against `34d2a91f9378568f36467f2f82fcb8f88709713b` on 2026-09-13. Scope: `PUBLIC_API_TRIM_PROMPT.md`. No commits, pushes, or GitHub changes.

## API decisions

| Candidate | Result and replacement |
| --- | --- |
| `CellSymbol` and `TerminalCell.symbol` | Private storage; read with `symbol()`. ASCII/inline/heap representation and the 24-byte symbol layout are unchanged. No demonstrated need for a new mutation API. |
| `TerminalCell::continuation_of` | Crate internal. Producers submit `new`/`wide` anchors; surface writes maintain continuations. |
| `FontFaces::select` | Removed. Production `resolve` remains private and unchanged. The oracle selects explicit fixture faces independently and rejects unsupported partial face chains. |
| `FontFaces::with_synthesis` | Removed; set `synthesize` directly or use struct update syntax. |
| `font_family` | Removed, including its single-purpose module and reexports. Use `FontSource::Family(name.into())`. |
| `RatatuiTerminal::drawn` | Removed; construct then `draw`. |
| `RatatuiTerminal::snapshot` | Removed; use `surface().snapshot()`. |
| `RatatuiTerminal::fit_to` | Removed; available space and resize policy belong to the application. Shared examples use `examples/common/app.rs::fit_grid`. |

The example fitting helper first adopts validated geometry with `set_geometry`, then calculates the grid and resizes only when needed. This also updates Ratatui pixel dimensions when fractional cell metrics or raster scale change without a grid resize. Loading, failed, foreign, and stale output cannot drive fitting. Resize invalidates the old geometry until the next measurement.

Retained after inspection:

- Raw `RatatuiBackend`, `from_surface`, and `RatatuiTerminal` support resource-owned consumers and Bevy components. `from_backend` and conversion from a raw terminal preserve integration choices. `with_renderer` pairs the correct surface, `draw` handles an infallible backend, and `resize_grid` synchronizes backend and Ratatui buffers. Their removal would disperse useful integration logic.
- `TerminalGeometry` accessors, `matches_surface`, `is_current`, surface identity/revision, and coherent `SurfaceInfo` reads express correctness contracts. They are not interchangeable getters over safely editable public fields.
- Transactional `SurfaceUpdate`, snapshots, row/cell/text readers, cursor controls, bounded caches, and diagnostic counters serve producers, renderer synchronization, and tests. Their implementation is unchanged.
- All sizing modes, `TerminalSizing::font`, and both grid calculations remain: explicit cell-size fitting works before measurement; geometry fitting uses validated, snapped metrics.
- Style builders, color constants, occupancy readers, and `FontFaces::regular` remain legible construction/read APIs. No additional abstraction was introduced just to reduce method counts.

## Consumer updates

Examples, README snippets, exports, tests, and font registration regression were updated. A shared test measurement helper avoids duplicating the app/font setup. No affected benchmark used the removed APIs; both benchmark fixtures compile.

Ratty's `ratty-terminal-main` worktree received direct family construction and `surface().snapshot()` replacements in `src/terminal.rs` and `examples/headless_snapshot.rs`. Another process subsequently edited the same worktree and built without the local dependency override, changing its lockfile during verification. Those concurrent changes are preserved. Verification therefore uses `/tmp/public-api-trim/ratty-consumer`, an isolated checkout of `71b576c9fa3722d312eb84d228919b0c1d23d669` with only our consumer replacements, and the explicit local override `/tmp/public-api-trim/ratty-patch.toml`. Its local resolved lockfile is verification evidence, not a publication dependency update.

The real `bevy_ratatui` fixture uses its pinned `f1118a98ab8aaacbed8f9a03e1f278871f39e84e` checkout and a resource-owned raw `ratatui::Terminal<RatatuiBackend>`. Its runtime test verifies drawing, resizing, measurement adoption, UI image binding, stable image identity, and no CPU image copy. Native-only and windowed configurations compile. This is a migration fixture, not the complete future application migration.

## Verification

Evidence directory: `/tmp/public-api-trim`. Exact commands and exit codes are in `verification.json`, `metal-gpu.json`, `vulkan-gpu.json`, `ratty-verification.json`, and `supplemental-verification.json`.

- `cargo fmt --all -- --check`: passed.
- `cargo check --workspace --all-targets --all-features --locked`: passed.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: passed.
- `cargo test --workspace --all-features --lib --tests --locked`: 84 core, 17 adapter, and 2 application-layout tests passed. GPU tests intentionally ignored by this command were run separately below. The unrelated ignored renderer benchmark was not used as a correctness check.
- `cargo test --all-features --example glyph_fidelity --locked`: 7 oracle tests passed, including rejection of partial face chains.
- `cargo test --all-features --example ratatui_examples --locked`: 23 passed.
- `cargo test --workspace --all-features --doc --locked`: 9 passed.
- `RUSTDOCFLAGS=-D warnings cargo doc --workspace --all-features --no-deps --locked`: passed.
- Core default and no-default-features all-target checks passed; adapter no-default-features all-target check passed. The standalone `/tmp/terminal-minimal-gpu` fixture also rendered and read back an image with core default features disabled, avoiding reliance solely on workspace dev-feature unification.
- Real context runtime test, windowed check, native-only consumer check, audit-workloads check, and renderer-comparison adapter check passed, all locked.
- GPU readback/export regressions: 6 passed on Metal and 6 on software Vulkan.
- Primary fidelity: Fit, natural 23, compact 23/0.85, and fixed 9×18/font 18; all six fonts and four scales on both backends. Each mode checked 18,720 available cells in 144 groups with zero failures. The 528 unavailable samples per mode remain explicitly classified; oversize ink is checked against deterministic cropping rather than silently skipped.
- Fallback/color/lifecycle coverage: all eight stages passed on both backends, 24 case/scale combinations per stage. Every primary and coverage `results.tsv` matches the previous verified baseline byte for byte (`fidelity-summary.json`). No pixel comparison, baseline, crop policy, or tolerance was weakened. Enlarged glyph and color captures were visually inspected as a supplementary check.
- Ratty locked all-target/all-feature check and strict Clippy passed with the local override. Workspace tests passed: 106 Ratty tests, 105 VT tests, and 1 doctest (3 existing ignored VT tests). The headless renderer and both standalone widget examples built. All seven Metal headless smoke captures passed with zero stale cells: natural, compact, missing-family fallback, short-lived child scene, inline document, style navigation, and 30-row scrolling. PNGs and input-delivery/content diagnostics are retained in `ratty-smoke`; style and inline-document captures were visually inspected.

## Separate full-diff review

Reviewed the complete tracked diff, untracked prompt and tests, and isolated Ratty consumer diff in a separate pass after implementation. Checked public visibility/reexports, construction and occupancy invariants, geometry rejection/adoption ordering, unchanged-grid pixel updates, resize invalidation, oracle independence, test coverage, documentation, and consumer replacements. Inspected the actual raw-context fixture and smoke assertions to establish their coverage.

The full check initially caught a wrong example module alias and a missing test crate documentation comment; both were fixed before the successful checks above. No confirmed correctness, regression, security, or unsafe-edge finding remains from the review. Surface transaction/cache code, renderer geometry implementation, production face resolution, font assets, and dependency manifests/lockfile are unchanged. No additional production changes were needed after review.

## Limits

VS15 text-presentation fallback remains the previously documented upstream Parley limitation. This API cleanup does not change the dependency or claim to fix that optional probe. Required fidelity coverage passes. GPU shutdown logs include closed readback-channel warnings after successful captures; completion assertions and process exits passed.

Consumer verification is local. Ratty needs a published library revision and a deliberate dependency update before these breaking API changes can be used without the local override. Concurrent Ratty edits are outside this review and were not reverted or claimed as verified. No remote CI or PR state was changed or used to claim completion.
