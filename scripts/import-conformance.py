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
    "missing-paths": ["supported"] * 6,
    "quoted-selectors": ["syntax", "syntax", "syntax", "syntax", "supported", "syntax", "supported", "supported"],
}
FLATTENING_SUPPORTED = {*(f"case{i:03}.json" for i in range(37)), "case034a.json"}

REASONS = {
    "supported": "Implemented paths, sequences, filters, scalar, aggregate or constructor semantics",
    "error": "Implemented compile/runtime error; local kind mapped from upstream code",
    "syntax": "Deferred expression syntax; see CONFORMANCE.md",
    "deferred": "Deferred builtin; asserts runtime UnsupportedExpression",
    "limit": "Exceeds the documented 64-call recursion guard; tail-call elimination deferred",
}

# Complete groups, with reviewed unsupported cases kept explicit.
EXPRESSION_GROUPS = {
    "variables": (13, set()),
    "blocks": (7, set()),
    "conditionals": (9, set()),
    "closures": (2, {0, 1}),
    "lambdas": (14, set()),
    "higher-order-functions": (3, set()),
    "function-boolean": (24, set()),
    "function-exists": (25, set()),
    "numeric-operators": (19, set()),
    "comparison-operators": (29, set()),
    "boolean-expresssions": (31, {16}),
    "literals": (20, set()),
    "null": (7, set()),
    "parentheses": (8, set()),
    "predicates": (4, set()),
    "simple-array-selectors": (23, set()),
    "multiple-array-selectors": (3, {0, 1, 2}),
    "function-count": (14, {2}),
    "function-sum": (7, {2}),
    # Upstream keeps both min and max in this group.
    "function-max": (27, {2, 16}),
    "array-constructor": (21, {15}),
    "object-constructor": (27, {8, 9, 10, 11, 13, 15, 17, 18, 19, 20, 21, 22, 25, 26}),
}
ERROR_KINDS = {
    "S0212": "UnsupportedExpression",
    "T1003": "TypeError", "D1009": "DuplicateKey",
    "T0410": "TypeError", "T0412": "TypeError",
    "D1001": "NumericRange", "T2001": "TypeError", "T2002": "TypeError",
    "T2009": "TypeError", "T2010": "TypeError", "S0102": "NumericRange",
    "S0103": "UnsupportedExpression", "S0104": "UnsupportedExpression",
}


DEFERRED_CALLS = {
    "boolean-expresssions": {29, 30}, "predicates": {3},
    "object-constructor": {12}, "conditionals": {3, 4, 5},
    "lambdas": {10, 11, 12},
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
            for i in (0, 1, 3, 4, 5, 6, 7):
                statuses[i] = "supported"
        if path.name == "sequence-of-arrays.json":
            statuses = ["supported", "supported", "syntax", "syntax"]
        yield path, statuses

    for group, (count, deferred) in EXPRESSION_GROUPS.items():
        names = sorted((suite / "groups" / group).glob("*.json"))
        extra = {"comparison-operators": ["deep-equals.json"], "literals": ["array-inputs.json"], "array-constructor": ["array-sequences.json"]}.get(group, [])
        expected = [f"case{i:03}.json" for i in range(count)] + extra
        assert [p.name for p in names] == sorted(expected), group
        for path in names:
            spec = json.loads(path.read_text())
            cases = spec if isinstance(spec, list) else [spec]
            if path.name == "deep-equals.json":
                assert len(cases) == 19
                statuses = ["supported"] * 19
            elif path.name == "array-inputs.json":
                assert len(cases) == 4
                statuses = ["supported"] * 4
            elif path.name == "array-sequences.json":
                statuses = ["syntax", "syntax", "supported", "supported", "syntax"]
            else:
                index = int(path.stem[4:])
                status = "syntax" if index in deferred else "error" if "code" in spec else "supported"
                if index in DEFERRED_CALLS.get(group, set()): status = "deferred"
                if group == "lambdas" and index in {6, 7, 8}: status = "limit"
                statuses = [status]
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
        for expression in path.parent.glob(path.stem + ".jsonata"):
            shutil.copyfile(expression, target.with_suffix(".jsonata"))
        spec = json.loads(path.read_text())
        cases = spec if isinstance(spec, list) else [spec]
        assert len(cases) == len(statuses), str(relative)
        for index, (case, status) in enumerate(zip(cases, statuses)):
            if case.get("dataset") is not None:
                datasets.add(case["dataset"])
            row = {"file": str(relative), "index": index, "status": status, "reason": REASONS[status]}
            if status == "supported" and path.parent.name in {"variables", "blocks", "conditionals", "lambdas", "higher-order-functions", "function-boolean", "function-exists"}:
                row["reason"] = "Implemented lexical/function semantics; host JSON bindings adapted to declarations"
            if status in {"deferred", "limit"}:
                row.update(phase="evaluate", kind="DepthLimit" if status == "limit" else "UnsupportedExpression")
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
