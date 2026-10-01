#!/usr/bin/env python3
"""Observe the pinned corpus through the CLI; observations are not classifications."""
import argparse
import json
import subprocess
from pathlib import Path

REVISION = "8ee4476f8a228bfc7a62979ae0a9c13a4043cd03"

def observe(source, cli, remaining):
    root = Path(__file__).resolve().parents[1]
    suite = source / "test/test-suite"
    assert subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip() == REVISION
    seen = {(r["file"], r["index"]) for r in json.loads((root / "tests/conformance/manifest.json").read_text())["cases"]} if remaining else set()
    for path in sorted((suite / "groups").glob("*/*.json")):
        spec = json.loads(path.read_text())
        for index, case in enumerate(spec if isinstance(spec, list) else [spec]):
            file = str(path.relative_to(suite))
            if (file, index) in seen:
                continue
            expr = case.get("expr") or (path.parent / case["expr-file"]).read_text()
            if case.get("bindings"):
                expr = "(" + "".join("$" + k + ":=" + json.dumps(v) + ";" for k, v in case["bindings"].items()) + expr + ")"
            syntax = (case.get("code") or case.get("error", {}).get("code", "")).startswith("S")
            data = case.get("data")
            if case.get("dataset"):
                data = json.loads((suite / "datasets" / (case["dataset"] + ".json")).read_text())
            if not syntax and "data" not in case and not case.get("dataset"):
                expr = "($$:=();$__jx_conformance_result:=();$__jx_conformance_missing[$__jx_conformance_result:=(" + expr + ")];$__jx_conformance_result)"
            # JSONata strings can contain lone UTF-16 surrogates; argv cannot.
            expr = expr.encode("utf-8", "backslashreplace").decode("utf-8")
            row = {"file": file, "index": index, "expr": expr, "expected": case.get("result"), "code": case.get("code") or case.get("error", {}).get("code"), "undefined": bool(case.get("undefinedResult"))}
            try:
                child = subprocess.run([str(cli), "--", expr], input=json.dumps(data), text=True, capture_output=True, timeout=5)
                items = [json.loads(line) for line in child.stdout.splitlines()]
                actual = items[0] if len(items) == 1 else items
                expected = row["expected"]
                if case.get("unordered") and items:
                    actual = sorted(actual, key=lambda x: json.dumps(x, sort_keys=True))
                    expected = sorted(expected, key=lambda x: json.dumps(x, sort_keys=True))
                row.update(returncode=child.returncode, stderr=child.stderr.strip(), actual=actual, items=items)
                row["match"] = child.returncode == 0 and (not items if row["undefined"] else bool(items) and actual == expected)
            except subprocess.TimeoutExpired:
                row.update(returncode=-1, stderr="timeout", match=False)
            yield row

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("cli", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--remaining", action="store_true")
    args = parser.parse_args()
    rows = list(observe(args.source.resolve(), args.cli.resolve(), args.remaining))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(rows, indent=2, ensure_ascii=True) + "\n")
    for group in sorted({r["file"].split("/")[1] for r in rows}):
        subset = [r for r in rows if r["file"].split("/")[1] == group]
        print(group, len(subset), "results agree:", sum(r["match"] for r in subset), "rejected:", sum(r["returncode"] != 0 for r in subset))

if __name__ == "__main__":
    main()
