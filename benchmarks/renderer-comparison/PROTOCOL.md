# Adapter protocol v1

An adapter is an independent executable. It accepts one workload/size run and
prints one compact JSON object as the final non-empty stdout line. Diagnostics
belong on stderr.

## CLI

Required shared flags:

```text
--cols U16
--rows U16
--cell-width F32
--cell-height F32
--font-size U32
--warmup U32
--frames U32
--workload static|sparse|dense_ascii|dense_styled|unicode
--gpu-sync true|false
```

An adapter must reject zero grids/measurement counts and invalid dimensions.
Warmup frames execute the identical path but are omitted from samples.

## Frame boundary

The synchronized v1 frame is:

```text
start total timer
  render canonical Ratatui workload through the backend
  perform adapter-owned CPU preparation/submission
  run one complete Bevy main/render-world update
  wait for all work on Bevy's WGPU device (when --gpu-sync=true)
stop total timer
```

Startup, renderer/device creation, system-font discovery, shader pipeline
compilation, initial glyph-cache seeding, and delayed component materialization
must happen before warmup. Cache growth caused by workload content during warmup
is intentionally realistic and shared by all frames.

Adapters must not use an OS window, swapchain, vsync, screenshot/readback, image
encoding, sleep, or frame limiter in a measured frame. A CPU renderer must
include the conversion and Bevy image upload required to present its output; a
direct renderer must submit to Bevy's device or document any secondary device.

## JSON

Top-level required fields:

- `schema_version` (integer `1`)
- `adapter` (identity, versions, `render_path`, interpretation `notes`)
- `config` (the normalized CLI values)
- `output_width`, `output_height` (actual renderer output)
- `machine` (`arch`, `os`, GPU name/backend/type, font)
- `samples` (one object per measured frame)
- `summary` (distribution per timing field)

Every sample contains integer nanoseconds:

```json
{
  "frame": 0,
  "draw_ns": 0,
  "prepare_ns": 0,
  "submit_ns": 0,
  "bevy_update_ns": 0,
  "gpu_wait_ns": 0,
  "total_ns": 0
}
```

Unused adapter-owned phases are zero, not omitted. `total_ns` is measured
independently rather than computed from phase sums, so driver overhead remains
visible. A phase distribution contains `mean_ns`, `stddev_ns`, `min_ns`,
`p50_ns`, `p95_ns`, `p99_ns`, and `max_ns`.

Percentiles use nearest rank over independently sorted values. Mean and standard
deviation are population statistics. JSON numbers for raw timings are integers;
means/deviations are floating point.

## Correctness gate

Before registering a renderer as enabled:

1. Confirm it consumes the shared font and canonical workload.
2. Confirm the actual grid and reported output dimensions.
3. Run each workload for more frames than its cache/pipeline initialization.
4. Visually validate representative output with that renderer's own capture
   mechanism outside measured frames.
5. Confirm `--gpu-sync=true` waits for its actual GPU device. If it uses a
   secondary device, add a second explicit wait and document it.
6. Verify release-mode check/clippy/tests and run the quick controller profile.
