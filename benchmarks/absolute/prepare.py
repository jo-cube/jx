#!/usr/bin/env python3
"""Build an isolated benchmark crate and copy the read-only actual jkq worker."""
import argparse,hashlib,json,shutil,subprocess,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'scripts'))
from benchmark_identity import cargo_artifact, hashes
p=argparse.ArgumentParser(description=__doc__)
modes=p.add_mutually_exclusive_group()
modes.add_argument('--default-only',action='store_true');modes.add_argument('--simd-only',action='store_true')
p.add_argument('destination',type=Path);p.add_argument('--jkq',type=Path,default=Path('../jkq'))
a=p.parse_args();root=Path(__file__).resolve().parents[2];out=a.destination.resolve();out.mkdir(parents=True,exist_ok=True)
def run(cmd):subprocess.run(cmd,cwd=root,check=True)
def build_record(build, command, name, mode):
    artifact, cargo = cargo_artifact(command, root, name)
    binary=Path(artifact['executable'])
    generated=hashes(build,[build/'Cargo.toml',build/'Cargo.lock',*(build/'src').rglob('*')])
    return dict(identity,mode=mode,binary=str(binary),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                generated_sha256=generated,cargo=cargo)
if not a.simd_only:
    run(['python3','scripts/check-jkq-execution.py']+(['--copy-only'] if a.default_only else [])+[str(a.jkq),str(out/'worker')])
identity=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
    dirty=bool(subprocess.check_output(['git','--no-optional-locks','status','--porcelain'],cwd=root)),
    files={str(f.relative_to(root)):hashlib.sha256(f.read_bytes()).hexdigest() for f in (root/'benchmarks/absolute').glob('*') if f.is_file()})
identity['files'].update(hashes(root,[root/'scripts/check-jkq-execution.py',root/'scripts/benchmark_identity.py']))
if not a.simd_only:identity['worker']=json.loads((out/'worker/identity.json').read_text())
if a.default_only:
    build=out/'default-harness';(build/'src').mkdir(parents=True,exist_ok=True)
    for name in ['fixtures.rs','handwritten.rs','metrics.rs']:shutil.copyfile(root/'benchmarks/absolute'/name,build/'src'/name)
    shutil.copyfile(root/'benchmarks/absolute/default.rs',build/'src/main.rs')
    shutil.copytree(out/'worker/reference',build/'src/reference',dirs_exist_ok=True,copy_function=shutil.copyfile)
    test=build/'src/reference/jsonata/tests.rs';test.write_text(test.read_text().replace('crate::transform::build_plan','crate::reference::build_plan'))
    (build/'Cargo.toml').write_text('[package]\nname="jx-absolute-default"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\njx={path='+json.dumps(str(root/'crates/jx'))+'}\nserde="=1.0.228"\nserde_json="=1.0.150"\nlibc="=0.2.186"\n[profile.release]\nlto="thin"\ncodegen-units=1\n')
    if (root/'benchmarks/absolute/Cargo.default.lock').exists():shutil.copyfile(root/'benchmarks/absolute/Cargo.default.lock',build/'Cargo.lock')
    record=build_record(build,['cargo','build','--manifest-path',str(build/'Cargo.toml'),'--release','--offline','--locked'],
                        'jx-absolute-default','default-only')
    record['features']=record['cargo']['engine']['features']
    (out/'default-build.json').write_text(json.dumps(record,indent=2)+'\n')
    print(record['binary']);raise SystemExit(0)
build=out/'harness';(build/'src').mkdir(parents=True,exist_ok=True)
for name in ['main.rs','fixtures.rs','handwritten.rs','metrics.rs','typed.rs','validation.rs','simd.rs','facade.rs','worker_trial.rs']:
    if (root/'benchmarks/absolute'/name).exists():shutil.copyfile(root/'benchmarks/absolute'/name,build/'src'/name)
if not a.simd_only:
    for module in ['reference','foundation']:shutil.copytree(out/'worker'/module,build/'src'/module,dirs_exist_ok=True,copy_function=shutil.copyfile)
    for module in ['reference','foundation']:
        test=build/'src'/module/'jsonata/tests.rs'
        test.write_text(test.read_text().replace('crate::transform::build_plan',f'crate::{module}::build_plan'))
    # Exact public-facade integration: only compilation selection differs from jkq.
    shutil.copytree(build/'src/reference',build/'src/facade_worker',dirs_exist_ok=True,copy_function=shutil.copyfile)
    for name in ['mod.rs','jsonata/tests.rs']:
        p=build/'src/facade_worker'/name
        p.write_text(p.read_text().replace('crate::reference::build_plan','crate::facade_worker::build_plan'))
(build/'src/simd_metrics.rs').write_text((root/'benchmarks/absolute/metrics.rs').read_text()+'\npub fn live_bytes()->i64 { LIVE.load(Relaxed) }\n')
# The independently built crate keeps benchmark dependencies out of production Cargo manifests.
(build/'Cargo.toml').write_text(f'''[package]
name="jx-absolute"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
jx={{path={json.dumps(str(root/'crates/jx'))}}}
serde={{version="=1.0.228",features=["derive"]}}
serde_json={{version="=1.0.150",features=["raw_value"]}}
libc="=0.2.186"
simd-json={{version="=0.17.3",default-features=false,features=["swar-number-parsing","runtime-detection"]}}
[[bin]]
name="simd-reference"
path="src/simd.rs"
[[bin]]
name="validation"
path="src/validation.rs"
[profile.release]
lto="thin"
codegen-units=1
''')
if (root/'benchmarks/absolute/Cargo.lock').exists():shutil.copyfile(root/'benchmarks/absolute/Cargo.lock',build/'Cargo.lock')
record=build_record(build,['cargo','build','--manifest-path',str(build/'Cargo.toml'),'--release','--offline','--locked']+
                    (['--bin','simd-reference'] if a.simd_only else []),
                    'simd-reference' if a.simd_only else 'jx-absolute','simd-only' if a.simd_only else 'comparison')
(out/('simd-build.json' if a.simd_only else 'build.json')).write_text(json.dumps(record,indent=2)+'\n')
print(record['binary'])
