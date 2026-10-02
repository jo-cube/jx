"""Attribute sampled self time from macOS sample trees, without double counting."""
import collections
import gzip
import json
import pathlib
import re

root = pathlib.Path(__file__).parent


def summarize(path):
    trace = gzip.open(path, "rt").read()
    tree = trace.split("Call graph:\n", 1)[1].split("\nTotal number in stack", 1)[0]
    nodes, stack = [], []
    for line in tree.splitlines():
        match = re.match(r"^([ +!:|]*)(\d+) (.+)$", line)
        if not match:
            continue
        indent, count, symbol = len(match[1]), int(match[2]), match[3]
        while stack and stack[-1]["indent"] >= indent:
            stack.pop()
        node = dict(indent=indent, count=count, symbol=symbol, children=0,
                    path=[n["symbol"] for n in stack] + [symbol])
        if stack:
            stack[-1]["children"] += count
        nodes.append(node)
        stack.append(node)
    categories, phases, self_counts = collections.Counter(), collections.Counter(), collections.Counter()
    total = 0
    for node in nodes:
        count = node["count"] - node["children"]
        assert count >= 0
        if not count:
            continue
        total += count
        symbol, ancestry = node["symbol"], " ".join(node["path"])
        self_counts[symbol] += count
        if any(name in symbol for name in ("malloc", "free", "alloc", "dealloc", "drop_in_place", "drop_glue", "clone")):
            category = "allocation/drop/clone"
        elif "4json4scan" in ancestry:
            category = "scanner"
            phase = "validation"
            if "9arguments" in ancestry or "8function4call" in ancestry:
                phase = "argument acquisition"
            elif "8callback" in ancestry or "4plan" in ancestry:
                phase = "body/candidate capture"
            phases[phase] += count
        elif any(name in ancestry for name in ("11fingerprint", "SipHasher", "3sip", "DefaultHasher")):
            category = "fingerprint"
        elif "Units" in symbol and "next" in symbol:
            category = "utf16/string decoding"
        elif "constant_data" in symbol or ("8constant" in symbol and "4find" in symbol):
            category = "lookup/search"
        elif "<unknown binary>" in symbol:
            category = "unsymbolized executable code"
        elif "jx_native" in ancestry or "jx_native" in symbol or "6native" in symbol:
            category = "native ABI/input acquisition"
        elif "4plan" in symbol:
            category = "plan"
        else:
            category = "other"
        categories[category] += count
    assert total == sum(n["count"] for n in nodes if "Thread_" in n["symbol"])
    return dict(samples=total,
                percent={k: round(100*v/total, 2) for k, v in categories.items()},
                scanner_phases_percent={k: round(100*v/total, 2) for k, v in phases.items()},
                top_self=[dict(symbol=k, samples=v, percent=round(100*v/total, 2))
                          for k, v in self_counts.most_common(16)])


paths = sorted(root.glob("profile-*.txt.gz"))
data = dict(method="Five-second, one-millisecond samples after the first timed sample; disjoint approximate self attribution. Native code may be unsymbolized; see full traces.", profiles={p.name[:-7]:summarize(p) for p in paths})
(root/"profiles.json").write_text(json.dumps(data,indent=2)+"\n")
for k,v in data["profiles"].items(): print(k,v["percent"])
