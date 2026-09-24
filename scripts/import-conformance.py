#!/usr/bin/env python3
"""Import reviewed upstream groups at a pinned revision. No network use."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

REVISION = "8ee4476f8a228bfc7a62979ae0a9c13a4043cd03"  # JSONata v2.2.0
GROUPS = {
    "fields": ["supported"] * 8,
    "missing-paths": ["supported"] * 5 + ["syntax"],
    "quoted-selectors": ["syntax", "syntax", "syntax", "syntax", "supported", "syntax", "supported", "supported"],
}
FLATTENING_SUPPORTED = {"case001.json", "case002.json", "case016.json", "case024.json",
                        "case026.json", "case028.json", "case030.json", "case032.json",
                        *(f"case{i:03}.json" for i in range(3, 9)),
                        "case034.json", "case034a.json", "case035.json", "case036.json"}
REASONS = {
    "supported": "Implemented paths, sequences, filters, scalar or aggregate semantics",
    "error": "Implemented compile/runtime error; local kind mapped from upstream code",
    "syntax": "Deferred expression syntax; see CONFORMANCE.md",
}

# Complete groups, with reviewed unsupported cases kept explicit.
EXPRESSION_GROUPS = {
    "numeric-operators": (19, {18}),
    "comparison-operators": (29, {26, 27, 28}),
    "boolean-expresssions": (31, {10, 11, 16, 27, 28, 29, 30}),
    "literals": (20, {18, 19}),
    "null": (7, {1, 2, 3, 6}),
    "parentheses": (8, set()),
    "predicates": (4, {3}),
    "simple-array-selectors": (23, {14}),
    "multiple-array-selectors": (3, {0, 1, 2}),
    "function-count": (14, {2, 3, 4, 5, 6, 8, 9, 10, 11, 13}),
    "function-sum": (7, {2}),
    # Upstream keeps both min and max in this group.
    "function-max": (27, {2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 16, 17, 18, 19, 20, 22, 23, 24, 25}),
}
ERROR_KINDS = {
    "T0410": "TypeError", "T0412": "TypeError",
    "D1001": "NumericRange", "T2001": "TypeError", "T2002": "TypeError",
    "T2009": "TypeError", "T2010": "TypeError", "S0102": "NumericRange",
    "S0103": "UnsupportedExpression", "S0104": "UnsupportedExpression",
}


def inventory(suite):
    for group, statuses in GROUPS.items():
        names = sorted((suite / "groups" / group).glob("*.json"))
        assert [p.name for p in names] == [f"case{i:03}.json" for i in range(len(statuses))], group
        for path, status in zip(names, statuses):
            yield path, [status]
    names = sorted((suite / "groups" / "flattening").glob("*.json"))
    expected = [f"case{i:03}.json" for i in range(46)] + ["case034a.json", "array-inputs.json", "large.json", "sequence-of-arrays.json"]
    assert [p.name for p in names] == sorted(expected), "flattening inventory changed"
    for path in names:
        spec = json.loads(path.read_text())
        cases = spec if isinstance(spec, list) else [spec]
        statuses = ["syntax"] * len(cases)
        if path.name in FLATTENING_SUPPORTED:
            statuses = ["supported"]
        if path.name == "array-inputs.json":
            for i in (0, 1, 3, 4, 5):
                statuses[i] = "supported"
        yield path, statuses

    for group, (count, deferred) in EXPRESSION_GROUPS.items():
        names = sorted((suite / "groups" / group).glob("*.json"))
        extra = {"comparison-operators": ["deep-equals.json"], "literals": ["array-inputs.json"]}.get(group, [])
        expected = [f"case{i:03}.json" for i in range(count)] + extra
        assert [p.name for p in names] == sorted(expected), group
        for path in names:
            spec = json.loads(path.read_text())
            cases = spec if isinstance(spec, list) else [spec]
            if path.name == "deep-equals.json":
                assert len(cases) == 19
                statuses = ["supported" if i in {6, *range(11, 19)} else "syntax" for i in range(19)]
            elif path.name == "array-inputs.json":
                assert len(cases) == 4
                statuses = ["syntax"] * 4
            else:
                index = int(path.stem[4:])
                statuses = ["syntax" if index in deferred else "error" if "code" in spec else "supported"]
            yield path, statuses


def main():
    source = Path(sys.argv[1]).resolve()
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if revision != REVISION:
        raise SystemExit(f"Expected upstream revision {REVISION}, got {revision}")
    destination = Path(__file__).resolve().parents[1] / "tests" / "conformance"
    suite = source / "test" / "test-suite"
    rows = []
    datasets = set()
    for path, statuses in inventory(suite):
        relative = path.relative_to(suite)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
        spec = json.loads(path.read_text())
        cases = spec if isinstance(spec, list) else [spec]
        assert len(cases) == len(statuses), str(relative)
        for index, (case, status) in enumerate(zip(cases, statuses)):
            if case.get("dataset") is not None:
                datasets.add(case["dataset"])
            row = {"file": str(relative), "index": index, "status": status, "reason": REASONS[status]}
            if status == "error":
                row.update(phase="compile" if case["code"].startswith("S") else "evaluate", kind=ERROR_KINDS[case["code"]])
            rows.append(row)
    (destination / "datasets").mkdir(parents=True, exist_ok=True)
    for dataset in sorted(datasets):
        shutil.copyfile(suite / "datasets" / f"{dataset}.json", destination / "datasets" / f"{dataset}.json")
    shutil.copyfile(source / "LICENSE", destination / "LICENSE")
    (destination / "manifest.json").write_text(json.dumps({"revision": REVISION, "cases": rows}, indent=2) + "\n")
    print(f"Imported {len(rows)} cases and {len(datasets)} datasets at {REVISION}")


if __name__ == "__main__":
    main()
