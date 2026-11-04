#!/usr/bin/env python3
"""Compare fallow JSON output against an expected.json fixture.

Usage:
    python3 verify-expected.py <actual.json> <expected.json>

The expected.json file contains a subset of fields. Each issue type is
compared by matching (path, export_name) tuples — line numbers, columns,
and span offsets are ignored since they may shift with code changes.

For circular dependencies, files and length are compared.
For duplicate exports, export_name and location paths are compared.

Prints "PASS" on success, or a list of mismatches on failure.
"""

import json
import sys


def normalize_exports(items):
    """Extract (path, export_name) tuples from unused_exports or unused_types."""
    result = set()
    for item in items:
        path = item.get("path", "")
        name = item.get("export_name", "")
        result.add((path, name))
    return result


def normalize_files(items):
    """Extract path set from unused_files."""
    return {item.get("path", "") for item in items}


def normalize_dependencies(items):
    """Extract package_name set from dependency arrays."""
    return {item.get("package_name", "") for item in items}


def normalize_circular(items):
    """Extract (sorted_files_tuple, length) from circular dependencies."""
    result = set()
    for item in items:
        files = tuple(sorted(item.get("files", [])))
        length = item.get("length", 0)
        result.add((files, length))
    return result


def normalize_duplicates(items):
    """Extract (export_name, sorted_location_paths) from duplicate exports."""
    result = set()
    for item in items:
        name = item.get("export_name", "")
        locs = tuple(sorted(loc.get("path", "") for loc in item.get("locations", [])))
        result.add((name, locs))
    return result


def normalize_export_aliases(items):
    """Extract (path, sorted_export_names) from duplicate export aliases."""
    return {
        (item.get("path", ""), tuple(sorted(item.get("export_names", []))))
        for item in items
    }


def compare_sets(actual_set, expected_set, label):
    """Compare two sets, returning a list of mismatch descriptions."""
    errors = []
    missing = expected_set - actual_set
    extra = actual_set - expected_set

    if missing:
        for item in sorted(missing):
            errors.append(f"  MISSING {label}: {item}")
    if extra:
        for item in sorted(extra):
            errors.append(f"  EXTRA {label}: {item}")
    return errors


def check_re_exports(actual_items, expected_items):
    """Check is_re_export flags where specified in expected."""
    errors = []
    expected_by_key = {}
    for item in expected_items:
        if "is_re_export" in item:
            key = (item.get("path", ""), item.get("export_name", ""))
            expected_by_key[key] = item["is_re_export"]

    if not expected_by_key:
        return errors

    actual_by_key = {}
    for item in actual_items:
        key = (item.get("path", ""), item.get("export_name", ""))
        actual_by_key[key] = item.get("is_re_export", False)

    for key, expected_val in expected_by_key.items():
        actual_val = actual_by_key.get(key)
        if actual_val is None:
            continue  # Already caught by set comparison
        if actual_val != expected_val:
            errors.append(
                f"  MISMATCH is_re_export for {key}: expected={expected_val}, actual={actual_val}"
            )
    return errors


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <actual.json> <expected.json>", file=sys.stderr)
        sys.exit(2)

    with open(sys.argv[1]) as f:
        actual = json.load(f)

    with open(sys.argv[2]) as f:
        expected = json.load(f)

    errors = []

    # Check total_issues count
    if "total_issues" in expected:
        actual_total = actual.get("total_issues", 0)
        expected_total = expected["total_issues"]
        if actual_total != expected_total:
            errors.append(
                f"  total_issues: expected={expected_total}, actual={actual_total}"
            )

    # Compare each issue type
    comparisons = [
        ("unused_files", normalize_files),
        ("unused_dependencies", normalize_dependencies),
        ("unused_dev_dependencies", normalize_dependencies),
        ("unresolved_imports", lambda items: {
            (i.get("path", ""), i.get("specifier", "")) for i in items
        }),
    ]

    for field, normalizer in comparisons:
        if field in expected:
            actual_set = normalizer(actual.get(field, []))
            expected_set = normalizer(expected[field])
            errors.extend(compare_sets(actual_set, expected_set, field))

    # Exports with (path, export_name) matching
    export_fields = ["unused_exports", "unused_types"]
    for field in export_fields:
        if field in expected:
            actual_set = normalize_exports(actual.get(field, []))
            expected_set = normalize_exports(expected[field])
            errors.extend(compare_sets(actual_set, expected_set, field))
            # Also check is_re_export where specified
            errors.extend(
                check_re_exports(actual.get(field, []), expected[field])
            )

    # Circular dependencies
    if "circular_dependencies" in expected:
        actual_set = normalize_circular(actual.get("circular_dependencies", []))
        expected_set = normalize_circular(expected["circular_dependencies"])
        errors.extend(compare_sets(actual_set, expected_set, "circular_dependencies"))

    # Duplicate exports
    if "duplicate_exports" in expected:
        actual_set = normalize_duplicates(actual.get("duplicate_exports", []))
        expected_set = normalize_duplicates(expected["duplicate_exports"])
        errors.extend(compare_sets(actual_set, expected_set, "duplicate_exports"))

    if "duplicate_export_aliases" in expected:
        actual_set = normalize_export_aliases(actual.get("duplicate_export_aliases", []))
        expected_set = normalize_export_aliases(expected["duplicate_export_aliases"])
        errors.extend(compare_sets(actual_set, expected_set, "duplicate_export_aliases"))

    if errors:
        print("FAIL")
        for err in errors:
            print(err)
        sys.exit(1)
    else:
        print("PASS")


if __name__ == "__main__":
    main()
