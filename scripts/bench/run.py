#!/usr/bin/env python3
"""Engine benchmark runner (docs/BENCHMARK.md).

Measures wall time and peak RSS (per-process VmHWM via /proc) of the real
release CLI in pipeline mode, plus scaling, target-size, concurrency and
cancel-latency scenarios. Writes target/bench/results.json and prints a
markdown report. --check compares against scripts/bench/baseline.json with
the 10% relative regression threshold (human review, not a release block).
"""
import json
import os
import platform
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
CORPUS = REPO / "target/bench/corpus"
RESULTS = REPO / "target/bench/results.json"
BASELINE = REPO / "scripts/bench/baseline.json"
WARM_RUNS = 5


def target_dir():
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        capture_output=True, text=True, cwd=REPO, check=True,
    )
    return Path(json.loads(out.stdout)["target_directory"])


CLI = target_dir() / "release/pdf-compressor-cli"
CANCEL_PROBE = target_dir() / "release/examples/cancel_latency"


def vmsample(pid, stop, peak):
    """Track the child's peak RSS; VmHWM is already the high-water mark."""
    status = Path(f"/proc/{pid}/status")
    while not stop.is_set():
        try:
            for line in status.read_text().splitlines():
                if line.startswith("VmHWM:"):
                    peak[0] = max(peak[0], int(line.split()[1]))
        except (FileNotFoundError, ProcessLookupError):
            return
        stop.wait(0.02)


def measure(args, input_bytes=None, capture_output=False):
    """Run the CLI in pipeline mode; return wall time, peak RSS, summary."""
    start = time.monotonic()
    proc = subprocess.Popen(
        args,
        stdin=subprocess.PIPE if input_bytes is not None else subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture_output else subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        cwd=REPO,
    )
    peak = [0]
    stop = threading.Event()
    sampler = threading.Thread(target=vmsample, args=(proc.pid, stop, peak), daemon=True)
    sampler.start()
    stdout, stderr = proc.communicate(input=input_bytes)
    stop.set()
    sampler.join()
    wall_ms = (time.monotonic() - start) * 1000
    summary = {}
    try:
        summary = json.loads(stderr.decode(errors="replace"))
    except json.JSONDecodeError:
        pass
    return {
        "wall_ms": round(wall_ms, 1),
        "peak_rss_mib": round(peak[0] / 1024, 1),
        "exit": proc.returncode,
        "summary": summary,
        "stdout": stdout,
    }


def compress(fixture, extra=None):
    args = [str(CLI), "compress", "-", "--stdout"]
    if extra:
        args += extra
    return measure(args, input_bytes=fixture.read_bytes())


def series(fixture, extra=None, runs=WARM_RUNS, capture_first=False):
    """One cold run (kept separate) + `runs` warm runs."""
    cold = compress(fixture, extra)
    cold_output = cold.pop("stdout") if not capture_first else None
    warm = [compress(fixture, extra) for _ in range(runs)]
    walls = [r["wall_ms"] for r in warm]
    return {
        "cold_wall_ms": cold["wall_ms"],
        "cold_peak_rss_mib": cold["peak_rss_mib"],
        "warm_wall_p50": statistics.median(walls),
        "warm_wall_p95": statistics.quantiles(walls, n=20)[-1] if len(walls) > 1 else walls[0],
        "warm_wall_min": min(walls),
        "peak_rss_mib": max(r["peak_rss_mib"] for r in warm),
        "summary": cold["summary"],
        "exit": cold["exit"],
    }


def main():
    check = "--check" in sys.argv
    fixtures = sorted(CORPUS.glob("*.pdf"))
    if not fixtures:
        raise SystemExit("no corpus — run scripts/bench/corpus.py first")
    results = {
        "environment": {
            "platform": platform.platform(),
            "cpu_count": os.cpu_count(),
            "total_ram_mib": int(Path("/proc/meminfo").read_text().splitlines()[0].split()[1]) // 1024,
            "rustc": subprocess.run(["rustc", "--version"], capture_output=True, text=True).stdout.strip(),
            "git": subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True, cwd=REPO).stdout.strip(),
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
        },
        "fixtures": {},
        "scaling": [],
        "target_size": None,
        "concurrency": [],
        "cancel": [],
    }

    def fixture(name):
        return CORPUS / f"{name}.pdf"

    for name in ["scan-photo-small", "scan-photo-medium", "scan-photo-large", "scan-bw-medium", "mixed-medium", "text-small"]:
        print(f"==> {name}", flush=True)
        results["fixtures"][name] = series(fixture(name))

    print("==> corrupt (must fail cleanly)", flush=True)
    bad = compress(fixture("corrupt"))
    results["fixtures"]["corrupt"] = {
        "wall_ms": bad["wall_ms"], "peak_rss_mib": bad["peak_rss_mib"],
        "exit": bad["exit"], "summary": bad["summary"],
    }

    for pages in (1, 5, 10, 20, 40, 80):
        name = f"scale-{pages:02d}"
        print(f"==> {name}", flush=True)
        entry = series(fixture(name), runs=3)
        entry["pages"] = pages
        results["scaling"].append(entry)

    print("==> target-size 2MB on scan-photo-large", flush=True)
    results["target_size"] = series(fixture("scan-photo-large"), extra=["--target-size", "2MB"], runs=3)

    print("==> concurrency (loaded, not a formal baseline)", flush=True)
    big = fixture("scan-photo-large").read_bytes()
    bw = fixture("scan-bw-medium").read_bytes()
    jobs = {}
    for label, data in [("scan-photo-large", big), ("scan-bw-medium", bw)]:
        args = [str(CLI), "compress", "-", "--stdout"]
        start = time.monotonic()
        proc = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=REPO)
        peak = [0]
        stop = threading.Event()
        sampler = threading.Thread(target=vmsample, args=(proc.pid, stop, peak), daemon=True)
        sampler.start()
        jobs[label] = (proc, start, stop, sampler, peak, data)
    for label, (proc, start, stop, sampler, peak, data) in jobs.items():
        proc.communicate(input=data)
        stop.set()
        sampler.join()
        results["concurrency"].append({
            "fixture": label,
            "wall_ms": round((time.monotonic() - start) * 1000, 1),
            "peak_rss_mib": round(peak[0] / 1024, 1),
            "exit": proc.returncode,
        })

    print("==> cancel latency", flush=True)
    probe = subprocess.run(
        [str(CANCEL_PROBE), str(fixture("scan-photo-large")), "250", "3"],
        capture_output=True, text=True, cwd=REPO, check=True,
    )
    results["cancel"] = [json.loads(line) for line in probe.stdout.splitlines() if line.startswith("{")]

    RESULTS.write_text(json.dumps(results, indent=2) + "\n")
    print_report(results)
    if check:
        regressions = compare_baseline(results)
        if regressions:
            print("\nREGRESSION REVIEW (>10%):", file=sys.stderr)
            for line in regressions:
                print(f"  {line}", file=sys.stderr)
            sys.exit(2)


def print_report(results):
    print("\n## Fixtures (cold kept separate; warm p50/p95)\n")
    print("| fixture | cold ms | warm p50 ms | warm p95 ms | peak RSS MiB |")
    print("| --- | ---: | ---: | ---: | ---: |")
    for name, entry in results["fixtures"].items():
        if "warm_wall_p50" in entry:
            print(f"| {name} | {entry['cold_wall_ms']:.0f} | {entry['warm_wall_p50']:.0f} | {entry['warm_wall_p95']:.0f} | {entry['peak_rss_mib']:.0f} |")
        else:
            print(f"| {name} (exit {entry['exit']}) | {entry['wall_ms']:.0f} | - | - | {entry['peak_rss_mib']:.0f} |")
    print("\n## Scaling (one unique image per page)\n")
    print("| pages | warm p50 ms | peak RSS MiB |")
    print("| ---: | ---: | ---: |")
    for entry in results["scaling"]:
        print(f"| {entry['pages']} | {entry['warm_wall_p50']:.0f} | {entry['peak_rss_mib']:.0f} |")
    target = results["target_size"]
    print(f"\n## Target size 2MB: p50 {target['warm_wall_p50']:.0f} ms, peak {target['peak_rss_mib']:.0f} MiB, summary {target['summary']}")
    print("\n## Concurrency (loaded)\n")
    for entry in results["concurrency"]:
        print(f"- {entry['fixture']}: {entry['wall_ms']:.0f} ms, peak {entry['peak_rss_mib']:.0f} MiB")
    print("\n## Cancel after 250 ms\n")
    for entry in results["cancel"]:
        print(f"- run {entry['run']}: outcome={entry['outcome']}, latency={entry['latency_ms']} ms, total={entry['total_ms']:.0f} ms")


def compare_baseline(results):
    if not BASELINE.exists():
        print(f"\nno baseline at {BASELINE} — skipping regression comparison")
        return []
    baseline = json.loads(BASELINE.read_text())
    lines = []
    for name, entry in results["fixtures"].items():
        old = baseline.get("fixtures", {}).get(name)
        if not old or "warm_wall_p50" not in entry:
            continue
        for key in ("warm_wall_p50", "peak_rss_mib"):
            before = old.get(key)
            if before and entry[key] > before * 1.10:
                lines.append(f"{name} {key}: {before} -> {entry[key]}")
    return lines


if __name__ == "__main__":
    main()
