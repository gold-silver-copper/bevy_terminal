#!/usr/bin/env python3
"""Build and run every headless Ratatui renderer adapter, then compare JSON results."""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import json
import os
import pathlib
import platform
import shlex
import subprocess
import sys
import tomllib
import math
from dataclasses import dataclass
from typing import Any


HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[1]
MANIFEST = HERE / "Cargo.toml"
REGISTRY = HERE / "adapters.toml"
TARGET_DIR = REPO_ROOT / "target" / "renderer-comparison"


PROFILES = {
    "quick": {
        "sizes": [(80, 24)],
        "workloads": ["static", "sparse", "dense_ascii", "unicode"],
        "warmup": 5,
        "frames": 20,
    },
    "standard": {
        "sizes": [(80, 24), (120, 40)],
        "workloads": ["static", "sparse", "dense_ascii", "dense_styled", "unicode"],
        "warmup": 30,
        "frames": 180,
    },
    "full": {
        "sizes": [(80, 24), (120, 40), (240, 54)],
        "workloads": ["static", "sparse", "dense_ascii", "dense_styled", "unicode"],
        "warmup": 120,
        "frames": 1000,
    },
}


@dataclass(frozen=True)
class RunSpec:
    adapter: dict[str, Any]
    workload: str
    cols: int
    rows: int
    repeat: int
    order_position: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=PROFILES, default="quick")
    parser.add_argument("--adapters", default="all", help="comma-separated IDs, or all")
    parser.add_argument("--workloads", help="comma-separated workload override")
    parser.add_argument("--sizes", help="comma-separated COLSxROWS override, e.g. 80x24,120x40")
    parser.add_argument("--warmup", type=int)
    parser.add_argument("--frames", type=int)
    parser.add_argument("--repeat", type=int, default=1)
    parser.add_argument(
        "--order-offset",
        type=int,
        default=0,
        help="rotate the deterministic adapter order by this many positions",
    )
    parser.add_argument("--cell-width", type=float, default=10.0)
    parser.add_argument("--cell-height", type=float, default=20.0)
    parser.add_argument(
        "--font-size",
        type=int,
        help="override the registry's per-adapter resolution-matched font size",
    )
    parser.add_argument("--gpu-sync", action=argparse.BooleanOptionalAction, default=True)
    parser.add_argument(
        "--captures",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="save one post-timing PNG for every successful adapter case",
    )
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--allow-failures", action="store_true")
    parser.add_argument("--timeout", type=int, default=900, help="seconds per adapter run")
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--list", action="store_true", help="print registry and exit")
    return parser.parse_args()


def load_registry() -> dict[str, Any]:
    with REGISTRY.open("rb") as stream:
        registry = tomllib.load(stream)
    if registry.get("schema_version") != 1:
        raise RuntimeError("unsupported adapters.toml schema")
    return registry


def selected_adapters(registry: dict[str, Any], selector: str) -> list[dict[str, Any]]:
    adapters = registry["adapter"]
    if selector == "all":
        return adapters
    requested = {item.strip() for item in selector.split(",") if item.strip()}
    known = {adapter["id"] for adapter in adapters}
    missing = requested - known
    if missing:
        raise RuntimeError(f"unknown adapters: {', '.join(sorted(missing))}")
    return [adapter for adapter in adapters if adapter["id"] in requested]


def parse_sizes(value: str | None, defaults: list[tuple[int, int]]) -> list[tuple[int, int]]:
    if value is None:
        return defaults
    sizes: list[tuple[int, int]] = []
    for item in value.split(","):
        cols, separator, rows = item.lower().partition("x")
        if not separator:
            raise RuntimeError(f"invalid size {item!r}; expected COLSxROWS")
        parsed = (int(cols), int(rows))
        if min(parsed) <= 0:
            raise RuntimeError("grid sizes must be positive")
        sizes.append(parsed)
    return sizes


def build(adapters: list[dict[str, Any]], environment: dict[str, str]) -> None:
    packages = [adapter["package"] for adapter in adapters if adapter["status"] == "enabled"]
    if not packages:
        return
    command = ["cargo", "build", "--release", "--manifest-path", str(MANIFEST)]
    for package in packages:
        command.extend(("--package", package))
    print("+", shlex.join(command), flush=True)
    subprocess.run(command, cwd=HERE, env=environment, check=True)


def binary_path(adapter: dict[str, Any]) -> pathlib.Path:
    suffix = ".exe" if os.name == "nt" else ""
    return TARGET_DIR / "release" / f"{adapter['binary']}{suffix}"


def source_files() -> list[pathlib.Path]:
    """Return inputs whose content can affect a benchmark binary or report."""
    files: list[pathlib.Path] = []
    excluded_parts = {".git", "results", "target", "__pycache__"}
    relevant_suffixes = {".lock", ".md", ".py", ".rs", ".sh", ".toml", ".wgsl"}
    for path in REPO_ROOT.rglob("*"):
        if not path.is_file() or excluded_parts.intersection(path.relative_to(REPO_ROOT).parts):
            continue
        if path.suffix in relevant_suffixes:
            files.append(path)
    return sorted(files)


def source_fingerprint(files: list[pathlib.Path]) -> str:
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.relative_to(REPO_ROOT).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def reject_stale_binaries(adapters: list[dict[str, Any]], files: list[pathlib.Path]) -> None:
    for adapter in adapters:
        adapter_dir = adapter["id"].replace("_", "-")
        relevant: list[pathlib.Path] = []
        for path in files:
            relative = path.relative_to(REPO_ROOT).as_posix()
            benchmark_relative = path.relative_to(HERE).as_posix() if path.is_relative_to(HERE) else ""
            if (
                benchmark_relative in {"Cargo.lock", "Cargo.toml"}
                or benchmark_relative.startswith("sdk/")
                or benchmark_relative.startswith(f"adapters/{adapter_dir}/")
                or benchmark_relative.startswith(f"patches/{adapter['id']}/")
                or (
                    adapter["id"] == "bevy_grid"
                    and (
                        relative in {"Cargo.lock", "Cargo.toml", "README.md"}
                        or relative.startswith("src/")
                    )
                )
            ):
                relevant.append(path)
        latest_source = max(path.stat().st_mtime_ns for path in relevant)
        binary = binary_path(adapter)
        if not binary.is_file():
            raise RuntimeError(f"missing release binary for {adapter['id']}: {binary}")
        if binary.stat().st_mtime_ns < latest_source:
            raise RuntimeError(
                f"stale release binary for {adapter['id']}; rebuild after the newest source change"
            )


def git_state() -> dict[str, Any]:
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=REPO_ROOT,
        check=True,
        text=True,
        capture_output=True,
    ).stdout.strip()
    dirty = bool(
        subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=REPO_ROOT,
            check=True,
            text=True,
            capture_output=True,
        ).stdout
    )
    return {"commit": commit, "dirty": dirty}


def validate_report(report: dict[str, Any], spec: RunSpec, args: argparse.Namespace) -> None:
    """Reject stale, incomplete, or internally inconsistent adapter output."""
    if report.get("schema_version") != 1:
        raise RuntimeError(f"{spec.adapter['id']} emitted an unsupported report schema")
    if report.get("adapter", {}).get("id") != spec.adapter["id"]:
        raise RuntimeError(f"{spec.adapter['id']} emitted a mismatched adapter identity")

    config = report.get("config", {})
    expected_config = {
        "cols": spec.cols,
        "rows": spec.rows,
        "cell_width": args.cell_width,
        "cell_height": args.cell_height,
        "font_size": effective_font_size(spec, args),
        "warmup_frames": args.warmup,
        "measured_frames": args.frames,
        "workload": spec.workload,
        "gpu_sync": args.gpu_sync,
    }
    for key, expected in expected_config.items():
        actual = config.get(key)
        matches = (
            isinstance(expected, float)
            and isinstance(actual, (int, float))
            and abs(float(actual) - expected) <= 1e-5
        ) or actual == expected
        if not matches:
            raise RuntimeError(
                f"{spec.adapter['id']} reported {key}={actual!r}, expected {expected!r}"
            )

    if min(report.get("output_width", 0), report.get("output_height", 0)) <= 0:
        raise RuntimeError(f"{spec.adapter['id']} reported an invalid output size")
    if args.font_size is None:
        expected_output = (
            round(spec.cols * args.cell_width),
            round(spec.rows * args.cell_height),
        )
        actual_output = (report["output_width"], report["output_height"])
        if actual_output != expected_output:
            raise RuntimeError(
                f"{spec.adapter['id']} produced {actual_output[0]}x{actual_output[1]} pixels "
                f"in matched-resolution mode, expected {expected_output[0]}x{expected_output[1]}"
            )
    samples = report.get("samples")
    if not isinstance(samples, list) or len(samples) != args.frames:
        raise RuntimeError(
            f"{spec.adapter['id']} emitted {len(samples) if isinstance(samples, list) else 0} "
            f"samples, expected {args.frames}"
        )

    timing_fields = (
        "draw_ns",
        "prepare_ns",
        "submit_ns",
        "bevy_update_ns",
        "gpu_wait_ns",
        "total_ns",
    )
    summary = report.get("summary", {})
    for index, sample in enumerate(samples):
        if sample.get("frame") != index:
            raise RuntimeError(f"{spec.adapter['id']} emitted a non-contiguous frame sequence")
        timings = [sample.get(field) for field in timing_fields]
        if any(not isinstance(value, int) or value < 0 for value in timings):
            raise RuntimeError(f"{spec.adapter['id']} emitted an invalid timing sample")
        if timings[-1] < sum(timings[:-1]):
            raise RuntimeError(f"{spec.adapter['id']} emitted inconsistent nested timings")
    if any(field not in summary for field in timing_fields):
        raise RuntimeError(f"{spec.adapter['id']} omitted timing distributions")


def run_one(
    spec: RunSpec,
    args: argparse.Namespace,
    environment: dict[str, str],
) -> dict[str, Any]:
    binary = binary_path(spec.adapter)
    command = [
        str(binary),
        "--cols",
        str(spec.cols),
        "--rows",
        str(spec.rows),
        "--cell-width",
        str(args.cell_width),
        "--cell-height",
        str(args.cell_height),
        "--font-size",
        str(effective_font_size(spec, args)),
        "--warmup",
        str(args.warmup),
        "--frames",
        str(args.frames),
        "--workload",
        spec.workload,
        "--gpu-sync",
        str(args.gpu_sync).lower(),
    ]
    capture_path = (
        args.capture_root
        / spec.adapter["id"]
        / spec.workload
        / f"{spec.cols}x{spec.rows}"
        / f"repeat-{spec.repeat}.png"
    )
    if args.captures:
        command.extend(("--capture", str(capture_path)))
    print("+", shlex.join(command), flush=True)
    completed = subprocess.run(
        command,
        cwd=HERE,
        env=environment,
        text=True,
        capture_output=True,
        timeout=args.timeout,
    )
    if completed.stderr:
        print(completed.stderr, file=sys.stderr, end="")
    if completed.returncode != 0:
        raise RuntimeError(
            f"{spec.adapter['id']} exited {completed.returncode}: {completed.stdout.strip()}"
        )
    if "panicked at" in completed.stderr or "Encountered a panic" in completed.stderr:
        raise RuntimeError(f"{spec.adapter['id']} reported a renderer panic on stderr")
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if not lines:
        raise RuntimeError(f"{spec.adapter['id']} emitted no JSON report")
    report = json.loads(lines[-1])
    validate_report(report, spec, args)
    report["controller"] = {
        "repeat": spec.repeat,
        "order_position": spec.order_position,
    }
    if args.captures:
        if not capture_path.is_file():
            raise RuntimeError(f"{spec.adapter['id']} did not write {capture_path}")
        report["controller"]["capture"] = str(capture_path)
        report["controller"]["capture_sha256"] = hashlib.sha256(
            capture_path.read_bytes()
        ).hexdigest()
    return report


def effective_font_size(spec: RunSpec, args: argparse.Namespace) -> int:
    """Return an explicit override or the adapter's calibrated font size."""
    if args.font_size is not None:
        return args.font_size
    return int(spec.adapter["matched_font_size"])


def row_for(report: dict[str, Any]) -> dict[str, Any]:
    config = report["config"]
    summary = report["summary"]
    total = summary["total_ns"]
    median_seconds = total["p50_ns"] / 1_000_000_000.0
    pixels = report["output_width"] * report["output_height"]
    return {
        "adapter": report["adapter"]["id"],
        "renderer_version": report["adapter"]["renderer_version"],
        "bevy_version": report["adapter"]["bevy_version"],
        "ratatui_version": report["adapter"]["ratatui_version"],
        "workload": config["workload"],
        "cols": config["cols"],
        "rows": config["rows"],
        "cell_width": config["cell_width"],
        "cell_height": config["cell_height"],
        "font_size": config["font_size"],
        "requested_pixels": f"{round(config['cols'] * config['cell_width'])}x{round(config['rows'] * config['cell_height'])}",
        "actual_pixels": f"{report['output_width']}x{report['output_height']}",
        "repeat": report["controller"]["repeat"],
        "frames": config["measured_frames"],
        "draw_p50_ms": summary["draw_ns"]["p50_ns"] / 1_000_000.0,
        "prepare_p50_ms": summary["prepare_ns"]["p50_ns"] / 1_000_000.0,
        "submit_p50_ms": summary["submit_ns"]["p50_ns"] / 1_000_000.0,
        "bevy_update_p50_ms": summary["bevy_update_ns"]["p50_ns"] / 1_000_000.0,
        "gpu_wait_p50_ms": summary["gpu_wait_ns"]["p50_ns"] / 1_000_000.0,
        "p50_ms": total["p50_ns"] / 1_000_000.0,
        "p95_ms": total["p95_ns"] / 1_000_000.0,
        "mean_ms": total["mean_ns"] / 1_000_000.0,
        "fps_p50": 1.0 / median_seconds,
        "mpix_s_p50": pixels / median_seconds / 1_000_000.0,
        "relative_to_fastest": 0.0,
        "gpu": report["machine"]["gpu_name"],
        "font": report["machine"]["font"],
    }


def normalize_rows(rows: list[dict[str, Any]]) -> None:
    fastest: dict[tuple[Any, ...], float] = {}
    for row in rows:
        key = (row["workload"], row["cols"], row["rows"], row["repeat"])
        fastest[key] = min(fastest.get(key, float("inf")), row["p50_ms"])
    for row in rows:
        key = (row["workload"], row["cols"], row["rows"], row["repeat"])
        row["relative_to_fastest"] = row["p50_ms"] / fastest[key]


def nearest_rank(values: list[int], quantile: float) -> int:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(quantile * len(ordered)) - 1)]


def aggregate_rows(reports: list[dict[str, Any]]) -> list[dict[str, Any]]:
    grouped: dict[tuple[Any, ...], list[dict[str, Any]]] = {}
    for report in reports:
        config = report["config"]
        key = (
            report["adapter"]["id"],
            config["workload"],
            config["cols"],
            config["rows"],
        )
        grouped.setdefault(key, []).append(report)

    rows: list[dict[str, Any]] = []
    for group in grouped.values():
        first = group[0]
        row = row_for(first)
        samples = [sample for report in group for sample in report["samples"]]
        totals = [sample["total_ns"] for sample in samples]
        row["repeat"] = "aggregate"
        row["frames"] = len(samples)
        for column, field in (
            ("draw_p50_ms", "draw_ns"),
            ("prepare_p50_ms", "prepare_ns"),
            ("submit_p50_ms", "submit_ns"),
            ("bevy_update_p50_ms", "bevy_update_ns"),
            ("gpu_wait_p50_ms", "gpu_wait_ns"),
        ):
            row[column] = nearest_rank([sample[field] for sample in samples], 0.5) / 1_000_000
        row["p50_ms"] = nearest_rank(totals, 0.5) / 1_000_000
        row["p95_ms"] = nearest_rank(totals, 0.95) / 1_000_000
        row["mean_ms"] = sum(totals) / len(totals) / 1_000_000
        median_seconds = row["p50_ms"] / 1_000
        pixels = first["output_width"] * first["output_height"]
        row["fps_p50"] = 1.0 / median_seconds
        row["mpix_s_p50"] = pixels / median_seconds / 1_000_000
        rows.append(row)
    normalize_rows(rows)
    return rows


def write_outputs(
    output: pathlib.Path,
    reports: list[dict[str, Any]],
    statuses: list[dict[str, Any]],
    failures: list[dict[str, Any]],
    command_line: list[str],
    build_fingerprint: str,
    repository: dict[str, Any],
) -> None:
    output.mkdir(parents=True, exist_ok=True)
    with (output / "results.jsonl").open("w", encoding="utf-8") as stream:
        for report in reports:
            stream.write(json.dumps(report, sort_keys=True) + "\n")

    rows = [row_for(report) for report in reports]
    normalize_rows(rows)
    aggregates = aggregate_rows(reports)
    if rows:
        with (output / "summary.csv").open("w", encoding="utf-8", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
    if aggregates:
        with (output / "aggregate.csv").open("w", encoding="utf-8", newline="") as stream:
            writer = csv.DictWriter(stream, fieldnames=list(aggregates[0]))
            writer.writeheader()
            writer.writerows(aggregates)

    metadata = {
        "schema_version": 1,
        "created_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "command": command_line,
        "platform": platform.platform(),
        "python": sys.version,
        "build_fingerprint": build_fingerprint,
        "repository": repository,
        "statuses": statuses,
        "failures": failures,
    }
    (output / "run.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
    write_markdown(output / "summary.md", rows, aggregates, statuses, failures)
    write_parity_markdown(output / "parity.md", aggregates)


def write_markdown(
    path: pathlib.Path,
    rows: list[dict[str, Any]],
    aggregates: list[dict[str, Any]],
    statuses: list[dict[str, Any]],
    failures: list[dict[str, Any]],
) -> None:
    lines = [
        "# Ratatui renderer comparison",
        "",
        "Headline time is synchronized end-to-end frame wall time. Lower is better. Relative values are scoped to one workload, grid, and repeat.",
        "Every row renders the same terminal cell count. The default calibrated font sizes make every backend render the requested 10x20 physical cells; `--font-size` explicitly opts into renderer-native dimensions.",
        "",
        "| adapter | workload | grid | repeat | cell/font px | actual px | p50 ms | p95 ms | FPS | MPix/s | relative |",
        "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for row in rows:
        lines.append(
            f"| {row['adapter']} | {row['workload']} | {row['cols']}x{row['rows']} | {row['repeat']} | "
            f"{row['cell_width']:g}x{row['cell_height']:g}/{row['font_size']} | "
            f"{row['actual_pixels']} | {row['p50_ms']:.3f} | {row['p95_ms']:.3f} | "
            f"{row['fps_p50']:.1f} | {row['mpix_s_p50']:.1f} | {row['relative_to_fastest']:.2f}x |"
        )
    if aggregates:
        lines.extend(
            (
                "",
                "## Aggregate raw samples",
                "",
                "Percentiles below are recomputed from every raw frame across repetitions; process percentiles are not averaged.",
                "",
                "| adapter | workload | grid | frames | cell/font px | actual px | p50 ms | p95 ms | relative |",
                "|---|---|---:|---:|---:|---:|---:|---:|---:|",
            )
        )
        for row in aggregates:
            lines.append(
                f"| {row['adapter']} | {row['workload']} | {row['cols']}x{row['rows']} | "
                f"{row['frames']} | {row['cell_width']:g}x{row['cell_height']:g}/{row['font_size']} | "
                f"{row['actual_pixels']} | {row['p50_ms']:.3f} | {row['p95_ms']:.3f} | "
                f"{row['relative_to_fastest']:.2f}x |"
            )
    unsupported = [status for status in statuses if status["status"] != "enabled"]
    if unsupported:
        lines.extend(("", "## Unsupported adapters", ""))
        for status in unsupported:
            lines.append(f"- `{status['id']}`: {status.get('reason', status['status'])}")
    if failures:
        lines.extend(("", "## Failures", ""))
        for failure in failures:
            lines.append(f"- `{failure['adapter']}`: {failure['error']}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_parity_markdown(path: pathlib.Path, rows: list[dict[str, Any]]) -> None:
    indexed = {
        (row["adapter"], row["workload"], row["cols"], row["rows"]): row for row in rows
    }
    cases = sorted(
        (workload, cols, grid_rows)
        for adapter, workload, cols, grid_rows in indexed
        if adapter == "bevy_grid"
        and ("bevy_tui_texture", workload, cols, grid_rows) in indexed
    )
    if not cases:
        path.write_text("# bevy_grid parity\n\nComparison pair was not present.\n", encoding="utf-8")
        return
    ratios: list[float] = []
    lines = [
        "# bevy_grid parity with bevy_tui_texture",
        "",
        "| workload | grid | bevy_grid p50/p95 ms | bevy_tui_texture p50/p95 ms | p50 ratio | p95 ratio | gate |",
        "|---|---:|---:|---:|---:|---:|---:|",
    ]
    all_cases_pass = True
    for workload, cols, grid_rows in cases:
        grid = indexed[("bevy_grid", workload, cols, grid_rows)]
        reference = indexed[("bevy_tui_texture", workload, cols, grid_rows)]
        p50_ratio = grid["p50_ms"] / reference["p50_ms"]
        p95_ratio = grid["p95_ms"] / reference["p95_ms"]
        ratios.append(p50_ratio)
        passed = p50_ratio <= 1.10 and p95_ratio <= 1.25
        all_cases_pass &= passed
        lines.append(
            f"| {workload} | {cols}x{grid_rows} | {grid['p50_ms']:.3f}/{grid['p95_ms']:.3f} | "
            f"{reference['p50_ms']:.3f}/{reference['p95_ms']:.3f} | {p50_ratio:.3f} | "
            f"{p95_ratio:.3f} | {'PASS' if passed else 'FAIL'} |"
        )
    geometric_mean = math.exp(sum(math.log(ratio) for ratio in ratios) / len(ratios))
    complete = all_cases_pass and geometric_mean <= 1.0
    lines.extend(
        (
            "",
            f"Geometric mean p50 ratio: **{geometric_mean:.3f}**.",
            "",
            f"Overall performance gate: **{'PASS' if complete else 'FAIL'}**.",
        )
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    args = parse_args()
    registry = load_registry()
    adapters = selected_adapters(registry, args.adapters)
    if args.list:
        for adapter in adapters:
            suffix = f": {adapter.get('reason', '')}" if adapter["status"] != "enabled" else ""
            print(f"{adapter['id']} [{adapter['status']}]{suffix}")
        return 0

    profile = PROFILES[args.profile]
    args.workloads = (
        [item.strip() for item in args.workloads.split(",") if item.strip()]
        if args.workloads
        else profile["workloads"]
    )
    args.sizes = parse_sizes(args.sizes, profile["sizes"])
    args.warmup = profile["warmup"] if args.warmup is None else args.warmup
    args.frames = profile["frames"] if args.frames is None else args.frames
    if min(args.frames, args.repeat) <= 0 or args.warmup < 0:
        raise RuntimeError("frames/repeat must be positive and warmup non-negative")
    if args.font_size is not None and args.font_size <= 0:
        raise RuntimeError("font size must be positive")

    timestamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    output = (args.output or (HERE / "results" / timestamp)).resolve()
    args.capture_root = output / "captures"
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(TARGET_DIR)
    environment.setdefault("RUST_LOG", "error")

    enabled = [adapter for adapter in adapters if adapter["status"] == "enabled"]
    statuses = [
        {key: adapter[key] for key in ("id", "name", "status", "reason") if key in adapter}
        for adapter in adapters
    ]
    if not args.no_build:
        build(enabled, environment)
    files = source_files()
    reject_stale_binaries(enabled, files)
    build_fingerprint = source_fingerprint(files)
    repository = git_state()

    reports: list[dict[str, Any]] = []
    failures: list[dict[str, Any]] = []
    case_index = 0
    for repeat in range(1, args.repeat + 1):
        for workload in args.workloads:
            for cols, rows in args.sizes:
                # Advance within each case across repetitions as well as between cases. The
                # standard profile has ten cases, which is divisible by the five enabled
                # adapters; using case_index alone would otherwise leave every adapter in the
                # same process position for all three repetitions.
                offset = (args.order_offset + case_index + repeat - 1) % max(len(enabled), 1)
                ordered = enabled[offset:] + enabled[:offset]
                for order_position, adapter in enumerate(ordered):
                    spec = RunSpec(
                        adapter,
                        workload,
                        cols,
                        rows,
                        repeat,
                        order_position,
                    )
                    try:
                        reports.append(run_one(spec, args, environment))
                    except Exception as error:  # Continue to leave a complete machine-readable run.
                        message = str(error)
                        print(f"ERROR: {adapter['id']}: {message}", file=sys.stderr)
                        failures.append(
                            {
                                "adapter": adapter["id"],
                                "workload": workload,
                                "cols": cols,
                                "rows": rows,
                                "repeat": repeat,
                                "order_position": order_position,
                                "error": message,
                            }
                        )
                case_index += 1

    write_outputs(
        output,
        reports,
        statuses,
        failures,
        sys.argv,
        build_fingerprint,
        repository,
    )
    print(f"wrote {output}")
    return 0 if not failures or args.allow_failures else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2) from error
