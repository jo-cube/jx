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
    "missing-paths": ["supported", "supported", "syntax", "syntax", "syntax", "syntax"],
    "quoted-selectors": ["syntax", "syntax", "syntax", "syntax", "supported", "syntax", "supported", "supported"],
}
FLATTENING_SUPPORTED = {"case001.json", "case002.json", "case016.json", "case024.json",
                        "case026.json", "case028.json", "case030.json", "case032.json"}
REASONS = {
    "supported": "Identity, field paths, array navigation and sequences",
    "syntax": "Deferred expression syntax; see CONFORMANCE.md",
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
            statuses[0] = "supported"
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
            rows.append({"file": str(relative), "index": index, "status": status, "reason": REASONS[status]})
    (destination / "datasets").mkdir(parents=True, exist_ok=True)
    for dataset in sorted(datasets):
        shutil.copyfile(suite / "datasets" / f"{dataset}.json", destination / "datasets" / f"{dataset}.json")
    shutil.copyfile(source / "LICENSE", destination / "LICENSE")
    (destination / "manifest.json").write_text(json.dumps({"revision": REVISION, "cases": rows}, indent=2) + "\n")
    print(f"Imported {len(rows)} cases and {len(datasets)} datasets at {REVISION}")


if __name__ == "__main__":
    main()
