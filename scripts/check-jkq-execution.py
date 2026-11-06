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
import sys
from benchmark_identity import cargo_artifact, hashes

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--copy-only', action='store_true',
                    help='copy the unmodified worker and source identity without building an adapter')
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
    for version in (['reference'] if args.copy_only else ['reference', 'foundation']):
        target = out/version/name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
metadata = dict(jkq=str(args.jkq.resolve()),
    revision=subprocess.check_output(['git','-C',str(args.jkq),'rev-parse','HEAD'],text=True).strip(),
    dirty=bool(subprocess.check_output(['git','--no-optional-locks','-C',str(args.jkq),'status','--porcelain'])),
    source_sha256=identity, worker_modes=['reference'] if args.copy_only else ['reference','foundation'])
if args.copy_only:
    (out/'identity.json').write_text(json.dumps(metadata,indent=2)+'\n')
    print(out/'reference')
    sys.exit(0)
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
projection = """                input
                    .write_json(
                        self.plan.drops.len() + self.plan.tombstones.len(),
                        &mut self.output,
                    )"""
if projection not in s:
    parser.error('comparison adapter requires the current PreparedInput.write_json worker; '
                 'use --copy-only to copy other worker versions unchanged')
s = s.replace(projection, """                self.plan.projection.as_ref().expect("projection")
                    .write_json(&input, &mut self.output)""")
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

p.write_text(s)
shutil.copy2(root/'crates/jx/benches/architecture/jkq.rs',out/'main.rs')
artifact, cargo = cargo_artifact(['cargo','build','-p','jx','--release','--locked'],root,'jx')
library = next(Path(path) for path in artifact['filenames'] if path.endswith('.rlib'))
dependencies = next(Path(path).parent for path in artifact['filenames'] if path.endswith('.rmeta'))
command = ['rustc','-O','-C','lto=thin','-C','codegen-units=1','--edition=2024','-A','dead_code',
    str(out/'main.rs'),'--extern',f'jx={library}','-L',f'dependency={dependencies}','-o',str(out/'worker')]
subprocess.run(command,check=True)
metadata.update(binary=str(out/'worker'),binary_sha256=hashlib.sha256((out/'worker').read_bytes()).hexdigest(),
    command=command,cargo=cargo,library_sha256=hashlib.sha256(library.read_bytes()).hexdigest(),
    generated_sha256=hashes(out,[out/'main.rs',*(out/'reference').rglob('*'),*(out/'foundation').rglob('*')]),
    adapter_sha256={name:hashlib.sha256((out/'foundation'/name).read_bytes()).hexdigest() for name in files})
(out/'identity.json').write_text(json.dumps(metadata,indent=2)+'\n')
print(out/'worker')
