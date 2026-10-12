# Fuzzing

libFuzzer targets for `bevy_terminal` and `bevy_terminal_ratatui`, driven by
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) on nightly.

| Target | What it drives | What it checks |
| --- | --- | --- |
| `grid` | `TerminalGrid` writes: cells, wide glyphs, clears, resizes, scrolls, cursor | A reference grid model, wide-glyph coherence, change reporting |
| `ratatui_backend` | `RatatuiBackend` directly and through `ratatui::Terminal` | Every cell and the cursor against `TestBackend` plus a model of a real terminal |
| `renderer` | A headless Bevy app: content, configs (including invalid values), fonts, terminals, frames | Measured geometry, image size, documented statuses, idle frames doing no work |

```sh
cargo install cargo-fuzz
cd fuzz
cargo +nightly fuzz run grid -s none -- -max_total_time=240 -len_control=0 -rss_limit_mb=4096
```

Run one worker at a time. `-s none` skips AddressSanitizer: the crates forbid
`unsafe`, and instrumented Bevy builds are slow. Known issues the targets skip
are marked `KNOWN ISSUE` or `Known issue` in the sources; remove the skip when
the issue is fixed. CI runs each target for one minute.
