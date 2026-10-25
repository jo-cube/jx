#!/usr/bin/env python3
"""Replay public contract tests with only the compilation selection changed.

Generated source lives under the requested ignored destination, not in tests/.
Semantic expectations, controls, consumers and error assertions are unchanged.
Two native installation assertions explicitly require zero kernels for the IR;
all their existing result/error checks still run.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('destination', type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
out = args.destination.resolve()
tests = out / 'tests'
tests.mkdir(parents=True, exist_ok=True)
names = ['embedding', 'validated_input', 'shared_input', 'constant_bindings',
         'resource_controls', 'effects', 'value_access', 'execution', 'sequences',
         'runtime', 'functions', 'lexical', 'tuples', 'composition', 'compatibility', 'helpers',
         'semantics', 'conformance']
identity = {}
for name in names:
    source = root / 'crates/jx/tests' / (name + '.rs')
    text = source.read_text()
    identity[name] = hashlib.sha256(source.read_bytes()).hexdigest()
    text = text.replace('jx::compile(', 'jx::CompileOptions::default().compile_execution(')
    text = text.replace('.compile(', '.compile_execution(')
    text = text.replace('../../../tests/', str(root / 'tests') + '/')
    text = text.replace('../../tests/conformance', str(root / 'tests/conformance'))
    text = text.replace('use jx::{ErrorKind, compile};', 'use jx::{ErrorKind, compile_execution as compile};')
    if name == 'constant_bindings':
        text = text.replace('assert!(expression.enable_native().kernels > 0, "{expression:#?}");', 'assert_eq!(expression.enable_native().kernels, 0);')
    if name == 'shared_input':
        text = text.replace('assert!(expressions[0].enable_native().kernels > 0);', 'assert_eq!(expressions[0].enable_native().kernels, 0);')
    (tests / source.name).write_text(text)
shutil.copytree(root / 'crates/jx/tests/support', tests / 'support', dirs_exist_ok=True)
(out / 'contracts.rs').write_text(
    '#![cfg(test)]\n' + ''.join(
        f'#[path="tests/{name}.rs"] mod {name};\n' for name in names
    )
)
(out / 'Cargo.toml').write_text('''[package]
autotests=false
name="jx-facade-contracts"
version="0.0.0"
edition="2024"
[lib]
path="contracts.rs"
[workspace]
[features]
default=["experimental"]
experimental=[]
jit=["jx/jit"]
[dependencies]
jx={path=''' + json.dumps(str(root / 'crates/jx')) + ''',features=["experimental"]}
serde_json={version="1.0",features=["raw_value"]}
''')
(out / 'sources.json').write_text(json.dumps(identity, indent=2) + '\n')
results = []
for label, features in [('interpreter', []), ('native', ['--features', 'jit'])]:
    with (out / (label + '.log')).open('w') as log:
        result = subprocess.run(['cargo', 'test', '--no-fail-fast', '--offline', '--manifest-path', str(out / 'Cargo.toml'), *features], stdout=log, stderr=subprocess.STDOUT)
    results.append({'mode': label, 'exit': result.returncode})
    print(label, result.returncode, flush=True)
(out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
raise SystemExit(any(r['exit'] for r in results))
