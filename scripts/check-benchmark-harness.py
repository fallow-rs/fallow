#!/usr/bin/env python3
"""Validate the CodSpeed benchmark harness stays deterministic.

This checks the CI matrix, Cargo bench declarations, and Criterion benchmark
names together so benchmark drift fails before CodSpeed receives a confusing
report. It intentionally uses only the Python standard library because it runs
in GitHub Actions before project dependencies are installed.
"""

from __future__ import annotations

import re
import json
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BENCH_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "bench.yml"
# The walltime instrument lives in its own workflow so each CodSpeed run carries
# a single instrument; see issue #2024.
TYPE_AWARE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "bench-type-aware.yml"
MATRIX_SCRIPT = REPO_ROOT / ".github" / "scripts" / "generate-benchmark-matrix.mjs"

FAST_JOB = "benchmark"
FULL_JOB = "benchmark-full"
EXPECTED_MODE = "simulation"
TYPE_AWARE_JOB = "benchmark-type-aware"
TYPE_AWARE_MODE = "walltime"
TYPE_AWARE_BENCH = REPO_ROOT / "tools" / "type-aware-sidecar" / "bench" / "session.mjs"
TYPE_AWARE_MANIFEST = REPO_ROOT / "tools" / "type-aware-sidecar" / "package.json"
MAX_BENCHES_PER_SHARD = 1_000
NOISY_FAST_TARGETS = {
    ("fallow-core", "scaling_analysis"),
    ("fallow-engine", "dupes_pipeline"),
}
LOCAL_ONLY_TARGETS = {
    ("fallow-benchmarks", "programmatic_commands"),
    ("fallow-core", "large_analysis"),
}
REQUIRED_FAST_TARGETS = {
    ("fallow-core", "analysis"),
    ("fallow-engine", "dupes_detect"),
    ("fallow-benchmarks", "programmatic_stable"),
    ("fallow-benchmarks", "representative_sources"),
    ("fallow-benchmarks", "component_config"),
    ("fallow-benchmarks", "component_engine"),
    ("fallow-benchmarks", "component_graph"),
    ("fallow-benchmarks", "component_output"),
}
REQUIRED_FULL_TARGETS = {
    ("fallow-core", "scaling_analysis"),
    ("fallow-engine", "dupes_pipeline"),
}


@dataclass(frozen=True)
class BenchTarget:
    job: str
    label: str
    package: str
    bench: str


def error(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)


def package_dir(package: str) -> Path:
    if package == "fallow-benchmarks":
        return REPO_ROOT / "crates" / "benchmarks"
    prefix = "fallow-"
    if not package.startswith(prefix):
        raise ValueError(f"unsupported benchmark package: {package}")
    return REPO_ROOT / "crates" / package.removeprefix(prefix)


def package_benches(package: str) -> set[str]:
    manifest = package_dir(package) / "Cargo.toml"
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    return {bench["name"] for bench in data.get("bench", [])}


def bench_file(package: str, bench: str) -> Path:
    return package_dir(package) / "benches" / f"{bench}.rs"


def extract_matrix_targets(text: str) -> list[BenchTarget]:
    targets: list[BenchTarget] = []
    current_job: str | None = None
    in_matrix = False
    current: dict[str, str] = {}

    for line in text.splitlines():
        job_match = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if job_match:
            current_job = job_match.group(1)
            in_matrix = False
            current = {}
            continue

        if current_job not in {FAST_JOB, FULL_JOB}:
            continue

        if re.match(r"^    steps:\s*$", line):
            in_matrix = False
            if current:
                targets.append(target_from(current_job, current))
                current = {}
            continue

        if re.match(r"^    strategy:\s*$", line):
            in_matrix = True
            continue

        if not in_matrix:
            continue

        item_match = re.match(r"^          - label:\s*(.+?)\s*$", line)
        if item_match:
            if current:
                targets.append(target_from(current_job, current))
            current = {"label": item_match.group(1)}
            continue

        field_match = re.match(r"^            (package|bench):\s*(.+?)\s*$", line)
        if field_match and current:
            current[field_match.group(1)] = field_match.group(2)

    if current_job in {FAST_JOB, FULL_JOB} and current:
        targets.append(target_from(current_job, current))

    return targets


def fast_targets_from_generator() -> list[BenchTarget]:
    try:
        output = subprocess.check_output(
            ["node", str(MATRIX_SCRIPT), "--all"],
            cwd=REPO_ROOT,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ValueError(f"failed to run benchmark matrix generator: {exc}") from exc

    try:
        rows = json.loads(output)
    except json.JSONDecodeError as exc:
        raise ValueError(f"benchmark matrix generator did not emit JSON: {exc}") from exc

    targets = []
    for row in rows:
        missing = [name for name in ("label", "package", "bench") if name not in row]
        if missing:
            raise ValueError(
                f"benchmark matrix generator row missing {', '.join(missing)}: {row}"
            )
        targets.append(
            BenchTarget(
                job=FAST_JOB,
                label=str(row["label"]),
                package=str(row["package"]),
                bench=str(row["bench"]),
            )
        )
    return targets


def target_from(job: str, fields: dict[str, str]) -> BenchTarget:
    missing = [name for name in ("label", "package", "bench") if name not in fields]
    if missing:
        raise ValueError(f"{job} matrix entry missing {', '.join(missing)}: {fields}")
    return BenchTarget(
        job=job,
        label=fields["label"],
        package=fields["package"],
        bench=fields["bench"],
    )


def benchmark_names(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    return re.findall(r'bench_function\(\s*"([^"]+)"', text)


def assert_codspeed_action_modes(text: str, type_aware_text: str) -> list[str]:
    errors: list[str] = []
    # Both workflows upload through the same action, so the pin is checked across
    # the pair: splitting them must not let one drift to another version.
    action_refs = re.findall(
        r"uses:\s*CodSpeedHQ/action@([^\s]+)", f"{text}\n{type_aware_text}"
    )
    if not action_refs:
        errors.append("benchmark workflows do not use CodSpeedHQ/action")
    elif len(set(action_refs)) != 1:
        errors.append(f"CodSpeed action refs differ: {sorted(set(action_refs))}")

    if f"  {TYPE_AWARE_JOB}:" not in type_aware_text:
        errors.append(f"type-aware workflow is missing {TYPE_AWARE_JOB}")
    elif re.findall(r"mode:\s*([^\s]+)", type_aware_text) != [TYPE_AWARE_MODE]:
        errors.append(f"{TYPE_AWARE_JOB} must use {TYPE_AWARE_MODE} mode")
    if TYPE_AWARE_JOB in text:
        errors.append(
            f"{TYPE_AWARE_JOB} must stay out of the bench workflow: a CodSpeed run "
            "carries one instrument"
        )

    modes = re.findall(r"mode:\s*([^\s]+)", text)
    codspeed_modes = [mode for mode in modes if mode in {"simulation", "walltime", "memory"}]
    if not codspeed_modes:
        errors.append("bench workflow does not declare a CodSpeed mode")
    elif any(mode != EXPECTED_MODE for mode in codspeed_modes):
        errors.append(f"Rust CodSpeed modes must all be {EXPECTED_MODE}: {codspeed_modes}")

    if "id-token: write" not in text:
        errors.append("bench workflow must keep OIDC id-token permission for CodSpeed shards")
    return errors


def validate_type_aware_benchmark(text: str) -> list[str]:
    errors: list[str] = []
    if not TYPE_AWARE_BENCH.is_file():
        errors.append(f"type-aware benchmark is missing at {TYPE_AWARE_BENCH}")
    else:
        source = TYPE_AWARE_BENCH.read_text(encoding="utf-8")
        for name in (
            "type-aware cold semantic analysis",
            "type-aware warm semantic session",
        ):
            if f'.add("{name}"' not in source:
                errors.append(f"type-aware benchmark is missing {name!r}")
        if "await suite.run()" not in source:
            errors.append("type-aware benchmark must await instrumented CodSpeed completion")
    manifest = json.loads(TYPE_AWARE_MANIFEST.read_text(encoding="utf-8"))
    if manifest.get("scripts", {}).get("bench") != "node bench/session.mjs":
        errors.append("type-aware package must expose the stable bench/session.mjs command")
    if "@codspeed/tinybench-plugin" not in manifest.get("devDependencies", {}):
        errors.append("type-aware benchmark must use CodSpeed's walltime-compatible Tinybench plugin")
    if "npm run bench --prefix tools/type-aware-sidecar" not in text:
        errors.append("type-aware CodSpeed job does not run the package benchmark")
    if "tools/type-aware-sidecar/**" not in text:
        errors.append("type-aware benchmark paths do not trigger the benchmark workflow")
    return errors


def validate_targets(targets: list[BenchTarget]) -> list[str]:
    errors: list[str] = []
    seen: set[tuple[str, str]] = set()
    benches_by_package: dict[str, set[str]] = {}

    for target in targets:
        key = (target.package, target.bench)
        if key in seen:
            errors.append(f"duplicate CodSpeed target in matrix: {target.package}/{target.bench}")
        seen.add(key)

        benches = benches_by_package.setdefault(target.package, package_benches(target.package))
        if target.bench not in benches:
            errors.append(
                f"{target.package}/{target.bench} is in bench.yml but not declared in Cargo.toml"
            )

        path = bench_file(target.package, target.bench)
        if not path.is_file():
            errors.append(f"{target.package}/{target.bench} has no benchmark file at {path}")
            continue

        names = benchmark_names(path)
        if len(names) == 0:
            errors.append(f"{target.package}/{target.bench} contains no bench_function calls")
        if len(names) > MAX_BENCHES_PER_SHARD:
            errors.append(
                f"{target.package}/{target.bench} has {len(names)} benchmarks, "
                f"above shard limit {MAX_BENCHES_PER_SHARD}"
            )

        if target.job == FAST_JOB and key in NOISY_FAST_TARGETS:
            errors.append(f"noisy target {target.package}/{target.bench} cannot run in fast PR job")

        if key in LOCAL_ONLY_TARGETS:
            errors.append(f"local-only target {target.package}/{target.bench} cannot run in CodSpeed CI")

        if target.bench == "programmatic_stable":
            unstable = [name for name in names if not name.startswith("stable_")]
            if unstable:
                errors.append(
                    "programmatic_stable benchmark names must use stable_ prefix: "
                    + ", ".join(unstable)
                )

    return errors


def validate_required_targets(targets: list[BenchTarget]) -> list[str]:
    fast_targets = {
        (target.package, target.bench) for target in targets if target.job == FAST_JOB
    }
    full_targets = {
        (target.package, target.bench) for target in targets if target.job == FULL_JOB
    }
    errors = []
    for package, bench in sorted(REQUIRED_FAST_TARGETS - fast_targets):
        errors.append(f"missing fast CodSpeed target: {package}/{bench}")
    for package, bench in sorted(REQUIRED_FULL_TARGETS - full_targets):
        errors.append(f"missing full CodSpeed target: {package}/{bench}")
    return errors


def validate_unique_names() -> list[str]:
    by_name: dict[str, list[Path]] = {}
    for path in sorted((REPO_ROOT / "crates").glob("*/benches/*.rs")):
        for name in benchmark_names(path):
            by_name.setdefault(name, []).append(path.relative_to(REPO_ROOT))

    duplicates = {name: paths for name, paths in by_name.items() if len(paths) > 1}
    errors = []
    for name, paths in sorted(duplicates.items()):
        joined = ", ".join(str(path) for path in paths)
        errors.append(f"duplicate bench_function name {name!r}: {joined}")
    return errors


def main() -> int:
    text = BENCH_WORKFLOW.read_text(encoding="utf-8")
    if not TYPE_AWARE_WORKFLOW.is_file():
        error(f"type-aware benchmark workflow is missing at {TYPE_AWARE_WORKFLOW}")
        return 1
    type_aware_text = TYPE_AWARE_WORKFLOW.read_text(encoding="utf-8")
    try:
        static_targets = extract_matrix_targets(text)
        targets = fast_targets_from_generator() + [
            target for target in static_targets if target.job == FULL_JOB
        ]
    except ValueError as exc:
        error(str(exc))
        return 1

    errors = []
    errors.extend(assert_codspeed_action_modes(text, type_aware_text))
    errors.extend(validate_targets(targets))
    errors.extend(validate_required_targets(targets))
    errors.extend(validate_unique_names())
    errors.extend(validate_type_aware_benchmark(type_aware_text))

    if not any(target.job == FAST_JOB for target in targets):
        errors.append("fast benchmark job has no matrix targets")
    if not any(target.job == FULL_JOB for target in targets):
        errors.append("full benchmark job has no matrix targets")

    if errors:
        for message in errors:
            error(message)
        return 1

    print("benchmark harness ok")
    for target in targets:
        names = benchmark_names(bench_file(target.package, target.bench))
        print(f"- {target.job}: {target.package}/{target.bench} ({len(names)} benches)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
