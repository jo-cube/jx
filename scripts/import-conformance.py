#!/usr/bin/env python3
"""Refresh the complete pinned language corpus using its reviewed manifest."""
import json
from pathlib import Path
import shutil
import subprocess
import sys

REVISION = "8ee4476f8a228bfc7a62979ae0a9c13a4043cd03"  # JSONata v2.2.0

def main():
    source = Path(sys.argv[1]).resolve()
    assert subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip() == REVISION
    destination = Path(__file__).resolve().parents[1] / "tests/conformance"
    manifest = json.loads((destination / "manifest.json").read_text())
    assert manifest["revision"] == REVISION
    suite = source / "test/test-suite"
    observed = set()
    datasets = set()
    groups = set()
    for path in sorted((suite / "groups").glob("*/*.json")):
        relative = str(path.relative_to(suite))
        groups.add(path.parent.name)
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
        spec = json.loads(path.read_text())
        cases = spec if isinstance(spec, list) else [spec]
        # Two URI fixtures embed lone surrogates in expression source. Rust strings
        # cannot hold these; the equivalent JSONata escape preserves their meaning.
        adapted = False
        for case in cases:
            if "expr" in case and any(0xd800 <= ord(ch) <= 0xdfff for ch in case["expr"]):
                case["expr"] = case["expr"].encode("utf-8", "backslashreplace").decode("utf-8")
                if "value" in case.get("error", {}):
                    case["error"]["value"] = case["error"]["value"].encode("utf-8", "backslashreplace").decode("utf-8")
                adapted = True
        if adapted:
            target.write_text(json.dumps(spec, indent=2) + "\n")
        for index, case in enumerate(cases):
            observed.add((relative, index))
            if "expr-file" in case:
                shutil.copyfile(path.parent / case["expr-file"], target.parent / case["expr-file"])
            if case.get("dataset"):
                datasets.add(case["dataset"])
    reviewed = {(row["file"], row["index"]) for row in manifest["cases"]}
    assert observed == reviewed, "Every upstream case needs a reviewed classification"
    assert len(observed) == manifest["corpus_cases"]
    assert len(groups) == manifest["corpus_groups"]
    (destination / "datasets").mkdir(exist_ok=True)
    for dataset in sorted(datasets):
        shutil.copyfile(suite / "datasets" / (dataset + ".json"), destination / "datasets" / (dataset + ".json"))
    shutil.copyfile(source / "LICENSE", destination / "LICENSE")
    print(f"Imported {len(observed)} classified cases in {len(groups)} groups at {REVISION}")

if __name__ == "__main__":
    main()
