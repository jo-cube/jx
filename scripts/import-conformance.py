#!/usr/bin/env python3
"""Import three complete upstream groups at a reviewed revision. No network use."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

REVISION = "8ee4476f8a228bfc7a62979ae0a9c13a4043cd03"  # JSONata v2.2.0
GROUPS = {
    "fields": ["supported", "supported", "array", "array", "array", "supported", "array", "array"],
    "missing-paths": ["supported", "supported", "syntax", "syntax", "syntax", "syntax"],
    "quoted-selectors": ["syntax", "syntax", "syntax", "syntax", "supported", "syntax", "array", "supported"],
}
REASONS = {
    "supported": "Identity/object path subset",
    "array": "M2: array mapping and result sequences",
    "syntax": "Deferred expression syntax; see CONFORMANCE.md",
}


def main():
    source = Path(sys.argv[1]).resolve()
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if revision != REVISION:
        raise SystemExit(f"Expected upstream revision {REVISION}, got {revision}")
    destination = Path(__file__).resolve().parents[1] / "tests" / "conformance"
    suite = source / "test" / "test-suite"
    cases = []
    datasets = set()
    for group, statuses in GROUPS.items():
        names = sorted((suite / "groups" / group).glob("*.json"))
        if [p.name for p in names] != [f"case{i:03}.json" for i in range(len(statuses))]:
            raise SystemExit(f"Unexpected case inventory: {group}")
        for path, status in zip(names, statuses):
            relative = path.relative_to(suite)
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
            case = json.loads(path.read_text())
            datasets.add(case["dataset"])
            cases.append({"file": str(relative), "status": status, "reason": REASONS[status]})
    (destination / "datasets").mkdir(parents=True, exist_ok=True)
    for dataset in sorted(datasets):
        shutil.copyfile(suite / "datasets" / f"{dataset}.json", destination / "datasets" / f"{dataset}.json")
    shutil.copyfile(source / "LICENSE", destination / "LICENSE")
    (destination / "manifest.json").write_text(json.dumps({"revision": REVISION, "cases": cases}, indent=2) + "\n")
    print(f"Imported {len(cases)} cases and {len(datasets)} datasets at {REVISION}")


if __name__ == "__main__":
    main()
