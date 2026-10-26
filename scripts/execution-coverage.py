#!/usr/bin/env python3
"""Track semantic families of legacy-success cases rejected at migration entry.

Groups describe the first reported gap, refined by the pinned upstream test family.
A case can require several mechanisms; this is not a proof of full family coverage.
Pass an explicit inventory for the comparison cohort; no historical archive is required.
"""
import argparse
from collections import Counter
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('current', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--entry-inventory', type=Path, required=True, help='flat upstream inventory defining the comparison cohort; use current for a current-only inventory')
parser.add_argument('--gate', type=Path, help='explicitly update the pinned Rust conformance admission inventory')
args = parser.parse_args()
entry = json.loads(args.entry_inventory.read_text())
current = json.loads(args.current.read_text())
assert entry['revision'] == current['revision']
updated = {row['id']: row for row in current['cases']}
if args.gate:
    gate = dict(revision=current['revision'], cases=[[r['id'],r['status']] for r in current['cases']])
    args.gate.write_text('{\n  \"revision\": '+json.dumps(gate['revision'])+',\n  \"cases\": [\n'+',\n'.join('    '+json.dumps(r) for r in gate['cases'])+'\n  ]\n}\n')
assert len(updated) == len(entry['cases']) == 1679


def group(row):
    reason = row.get('reason', '')
    family = row['id'].split('/')[1]
    if args.entry_inventory and reason:
        return reason.split(' at byte')[0]
    if 'predicate may select numeric positions' in reason:
        return 'candidate selection and positions'
    if 'sequence producer' in reason:
        return 'path composition and candidate contexts'
    if 'immediate lambda' in reason or 'callback signature/arity' in reason:
        return 'general calls and retained environments'
    if 'unresolved binding' in reason:
        return 'lexical names and missing values'
    if family in {'lambdas', 'closures', 'higher-order-functions', 'function-signatures',
                  'function-applications', 'partial-application', 'tail-recursion',
                  'hof-reduce', 'hof-map', 'hof-filter', 'hof-single', 'hof-zip-map',
                  'function-each', 'function-sift', 'function-boolean', 'function-typeOf',
                  'function-keys', 'function-spread', 'function-string', 'comments',
                  'default-operator', 'function-exists'}:
        return 'general calls and retained environments'
    if family in {'joins', 'parent-operator'}:
        return 'tuples and ancestry'
    if family in {'regex', 'matchers'}:
        return 'matcher values and callbacks'
    if family.startswith(('function-format', 'function-parse', 'function-from', 'function-to')):
        return 'formatting and clock-dependent library'
    if family == 'function-eval':
        return 'dynamic compilation and lifetime regions'
    if family == 'function-shuffle':
        return 'evaluation effects'
    if family == 'range-operator':
        return 'range sequences'
    if family in {'function-assert', 'function-error'}:
        return 'diagnostic value kernels'
    if family == 'object-constructor':
        return 'grouping and dynamic constructors'
    if family in {'sorting', 'function-sort'}:
        return 'ordering and callback comparison'
    if family == 'transforms':
        return 'transform identity and updates'
    if family in {'wildcards', 'descendent-operator'}:
        return 'wildcard and recursive navigation'
    if family in {'simple-array-selectors', 'multiple-array-selectors', 'predicates',
                  'conditionals', 'string-concat', 'coalescing-operator', 'missing-paths'}:
        return 'candidate selection and positions'
    return 'path composition and candidate contexts'


rows = []
for row in entry['cases']:
    if row['expected_status'] != 'supported' or row['status'] == 'supported':
        continue
    actual = updated[row['id']]
    rows.append(dict(id=row['id'], group=group(row), entry_status=row['status'],
                     entry_reason=row.get('reason'), current_status=actual['status'],
                     current_reason=actual.get('reason')))
assert len({row["id"] for row in rows}) == len(rows)
summary = {}
for category in sorted({row['group'] for row in rows}):
    cases = [row for row in rows if row['group'] == category]
    summary[category] = dict(original=len(cases), **Counter(row['current_status'] for row in cases))
result = dict(revision=entry['revision'], method='First reported gap, refined by upstream semantic family; overlapping dependencies remain.',
              entry_counts=entry['counts'], current_counts=current['counts'],
              original_legacy_success_gaps=len(rows), groups=summary,
              first_current_gap=dict(Counter((row["current_reason"] or row["current_status"]).split(" at byte")[0] for row in rows if row["current_status"] != "supported")), cases=rows)
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(summary, indent=2))
