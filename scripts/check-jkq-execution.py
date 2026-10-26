#!/usr/bin/env python3
"""Build the actual jkq transform worker and a lower-level common-IR adapter.

No sibling writes, Kafka dependency, network, or runtime backend trait. Both
workers compile in one standalone release binary against the same common-IR engine.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('jkq', type=Path)
parser.add_argument('destination', type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
out = args.destination.resolve()
out.mkdir(parents=True, exist_ok=True)
source = args.jkq.resolve() / 'src/transform'
files = ['mod.rs', 'jsonata.rs', 'jsonata/tests.rs']
identity = {}
for name in files:
    data = (source/name).read_bytes()
    identity[name] = hashlib.sha256(data).hexdigest()
    for version in ['reference', 'foundation']:
        target = out/version/name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
p = out/'foundation/mod.rs'
s = p.read_text().replace('use jx::{CompileOptions, Expression, InputPlan, OwnedValue, ValueType};',
    'use jx::{CompileOptions, OwnedValue, ValueType};\nuse jx::execution::{Program as Expression, Acquisition as InputPlan};')
s = s.replace("InputPlan<'_>", 'InputPlan').replace('.compile(source)', '.compile_program(source)')
p.write_text(s)
p = out/'foundation/jsonata.rs'
s = p.read_text().replace('use jx::{Evaluation, InputPlan, PreparedInput};', 'use jx::Evaluation;\nuse jx::execution::{Acquisition, Prepared};')
s = s.replace("    input_plan: &'a InputPlan<'a>,", '    acquisition: Acquisition,')
s = s.replace("        input_plan: &'a InputPlan<'a>,\n", '')
s = s.replace('            input_plan,', '            acquisition: plan.input_plan(),')
s = s.replace('        acquisition: Acquisition,\n', '        input_plan: &Acquisition,\n').replace('.as_raw()', '.raw()')
s = s.replace('            acquisition: plan.input_plan(),', '            acquisition: input_plan.clone(),')
s = s.replace('self.input_plan.prepare(payload)', 'self.acquisition.prepare(payload)')
s = s.replace('self.evaluate_predicates(&input)', 'Self::evaluate_predicates(self.plan, &input)')
s = s.replace('self.tombstone_action()', 'Self::tombstone_action(self.plan)')
s = s.replace('''                input
                    .evaluate(self.plan.drops.len() + self.plan.tombstones.len())
                    .map_err(|error| error.to_string())
                    .and_then(|evaluation| serialize_projection(evaluation, &mut self.output)),''', '''                self.plan.projection.as_ref().expect("projection")
                    .write_json(&input, &mut self.output)
                    .map_err(|error| error.to_string())
                    .and_then(|present| if present { Ok(()) } else { Err("projection emitted no results".to_owned()) }),''')
s = s.replace('''        &self,
        input: &PreparedInput<'_, 'a, '_>,''', '''        plan: &TransformPlan,
        input: &Prepared<'_, '_>,''')
s = s.replace('for index in 0..self.plan.drops.len()', 'for index in 0..plan.drops.len()')
s = s.replace('for index in 0..self.plan.tombstones.len()', 'for index in 0..plan.tombstones.len()')
s = s.replace('self.predicate(input, index,', 'Self::predicate(plan, input, index,')
s = s.replace('''if self.predicate(
                input,
                self.plan.drops.len() + index,''', '''if Self::predicate(
                plan,
                input,
                plan.drops.len() + index,''')
s = s.replace('''return Ok(Some(Self::tombstone_action(self.plan)));''', '''return Ok(Some(Self::tombstone_action(plan)));''')
s = s.replace('''        let value = input
            .evaluate(expression_index)''', '''        let value = plan.drops.iter().chain(&plan.tombstones).chain(&plan.projection)
            .nth(expression_index).expect("expression index").evaluate(input)''')
s = s.replace('fn tombstone_action(&self) -> Action {\n        if self.plan.drop_tombstones', 'fn tombstone_action(plan: &TransformPlan) -> Action {\n        if plan.drop_tombstones')

# Keep the reference serializer for review, but no unused duplicate in adapter.
a=s.index('fn serialize_projection(');b=s.index('fn evaluation_result(',a);s=s[:a]+s[b:]
p.write_text(s)
shutil.copy2(root/'crates/jx/benches/architecture/jkq.rs',out/'main.rs')
subprocess.run(['cargo','build','-p','jx','--release','--locked'],cwd=root,check=True)
subprocess.run(['rustc','-O','-C','lto=thin','-C','codegen-units=1','--edition=2024','-A','dead_code',
    str(out/'main.rs'),'--extern',f'jx={root}/target/release/libjx.rlib','-L',f'dependency={root}/target/release/deps','-o',str(out/'worker')],check=True)
metadata = dict(jkq=str(args.jkq.resolve()),revision=subprocess.check_output(['git','-C',str(args.jkq),'rev-parse','HEAD'],text=True).strip(),
    source_sha256=identity,binary_sha256=hashlib.sha256((out/'worker').read_bytes()).hexdigest())
(out/'identity.json').write_text(json.dumps(metadata,indent=2)+'\n')
print(out/'worker')
