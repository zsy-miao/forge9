#!/usr/bin/env python3
"""Summarize one PresentMon stream, frame_ms CSV, or Metal HUD log."""

import argparse
import csv
import json
import math
import statistics
import sys
from contextlib import nullcontext
from pathlib import Path

METRICS = {
    "present_ms": ("MsBetweenPresents", "frame_ms"),
    "display_ms": ("MsBetweenDisplayChange",),
    "cpu_busy_ms": ("MsCPUBusy",),
    "cpu_wait_ms": ("MsCPUWait",),
    "gpu_busy_ms": ("MsGPUBusy",),
    "gpu_wait_ms": ("MsGPUWait",),
}


def summarize(csv_file, process_id=None, swap_chain=None):
    reader = csv.DictReader(csv_file)
    fields = reader.fieldnames or []
    columns = {
        name: next((column for column in aliases if column in fields), None)
        for name, aliases in METRICS.items()
    }
    if columns["present_ms"] is None:
        raise ValueError("Expected MsBetweenPresents or frame_ms")
    for expected, column in ((process_id, "ProcessID"), (swap_chain, "SwapChainAddress")):
        if expected is not None and column not in fields:
            raise ValueError(f"Missing {column} column for requested filter")

    samples = {name: [] for name, column in columns.items() if column}
    stream = None
    for row in reader:
        current = (row.get("ProcessID"), row.get("SwapChainAddress"))
        if process_id is not None and current[0] != process_id:
            continue
        if swap_chain is not None and current[1] != swap_chain:
            continue
        if stream is None:
            stream = current
        elif current != stream:
            raise ValueError("Multiple streams in capture. Filter by --process-id and --swap-chain")

        for name in samples:
            value = row.get(columns[name])
            if value is None or not value.strip() or value.strip().upper() in {"NA", "N/A"}:
                continue
            try:
                ms = float(value)
            except ValueError as error:
                raise ValueError(f"Invalid {columns[name]} value on line {reader.line_num}") from error
            if math.isfinite(ms) and (ms > 0 or ms == 0 and name not in {"present_ms", "display_ms"}):
                samples[name].append(ms)

    return summarize_samples(samples)


def summarize_metal_hud(log_file):
    samples = {"present_ms": [], "metal_gpu_time_ms": []}
    next_frame = 0
    for line_number, line in enumerate(log_file, 1):
        _, marker, data = line.partition("metal-HUD:")
        if not marker:
            continue
        # First frame number and two summary fields precede (present ms, GPU ms) pairs.
        fields = data.strip().split(",")
        try:
            first_frame = int(fields[0])
            timings = [float(field) for field in fields[3:]]
        except ValueError as error:
            raise ValueError(f"Invalid Metal HUD record on line {line_number}") from error
        if len(fields) < 3 or len(timings) % 2:
            raise ValueError(f"Incomplete Metal HUD record on line {line_number}")
        if first_frame < next_frame:
            raise ValueError("Overlapping Metal HUD frames. Use a log from one process and run")
        next_frame = first_frame + len(timings) // 2
        samples["present_ms"].extend(ms for ms in timings[::2] if math.isfinite(ms) and ms > 0)
        samples["metal_gpu_time_ms"].extend(ms for ms in timings[1::2] if math.isfinite(ms) and ms >= 0)
    return summarize_samples(samples)


def summarize_samples(samples):
    presents = samples["present_ms"]
    if not presents:
        raise ValueError("No positive presentation intervals in the selected stream")
    summary = {"presents_per_second": round(1000 / statistics.mean(presents), 3)}
    for name, values in samples.items():
        if not values:
            continue
        values.sort()
        summary[name] = {
            "samples": len(values),
            "p50": round(statistics.median(values), 3),
            "p95": round(values[math.ceil(0.95 * len(values)) - 1], 3),
            "p99": round(values[math.ceil(0.99 * len(values)) - 1], 3),
        }
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path, help="Capture path, or - for standard input")
    parser.add_argument("--process-id")
    parser.add_argument("--swap-chain")
    parser.add_argument("--metal-hud", action="store_true", help="Read a Metal HUD log from one process")
    args = parser.parse_args()
    if args.metal_hud and (args.process_id is not None or args.swap_chain is not None):
        parser.error("Filter Metal HUD logs by process before passing them to this script")
    try:
        source = nullcontext(sys.stdin) if args.capture == Path("-") else args.capture.open(encoding="utf-8-sig", newline="")
        with source as capture:
            result = summarize_metal_hud(capture) if args.metal_hud else summarize(capture, args.process_id, args.swap_chain)
    except (OSError, UnicodeError, ValueError, csv.Error) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
