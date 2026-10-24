#!/usr/bin/env python3
"""Build an isolated benchmark crate and copy the read-only actual jkq worker."""
import argparse,hashlib,json,shutil,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--default-only',action='store_true');p.add_argument('--simd-only',action='store_true');p.add_argument('destination',type=Path);p.add_argument('--jkq',type=Path,default=Path('../jkq'))
a=p.parse_args();root=Path(__file__).resolve().parents[2];out=a.destination.resolve();out.mkdir(parents=True,exist_ok=True)
def run(cmd):subprocess.run(cmd,cwd=root,check=True)
run(['python3','scripts/check-jkq-execution.py',str(a.jkq),str(out/'worker')])
if a.default_only:
    if a.simd_only:p.error('choose one build mode')
    build=out/'default-harness';(build/'src').mkdir(parents=True,exist_ok=True)
    for name in ['fixtures.rs','handwritten.rs','metrics.rs']:shutil.copy2(root/'benchmarks/absolute'/name,build/'src'/name)
    shutil.copy2(root/'benchmarks/absolute/default.rs',build/'src/main.rs')
    shutil.copytree(out/'worker/reference',build/'src/reference',dirs_exist_ok=True)
    test=build/'src/reference/jsonata/tests.rs';test.write_text(test.read_text().replace('crate::transform::build_plan','crate::reference::build_plan'))
    (build/'Cargo.toml').write_text('[package]\nname="jx-absolute-default"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\njx={path='+json.dumps(str(root/'crates/jx'))+'}\nserde="=1.0.228"\nserde_json="=1.0.150"\nlibc="=0.2.186"\n[profile.release]\nlto="thin"\ncodegen-units=1\n')
    if (root/'benchmarks/absolute/Cargo.default.lock').exists():shutil.copy2(root/'benchmarks/absolute/Cargo.default.lock',build/'Cargo.lock')
    run(['cargo','build','--manifest-path',str(build/'Cargo.toml'),'--release','--offline','--locked'])
    binary=build/'target/release/jx-absolute-default'
    (out/'default-build.json').write_text(json.dumps(dict(binary=str(binary),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),features=[]),indent=2)+'\n')
    print(binary);raise SystemExit(0)
build=out/'harness';(build/'src').mkdir(parents=True,exist_ok=True)
for module in ['reference','foundation']:shutil.copytree(out/'worker'/module,build/'src'/module,dirs_exist_ok=True)
for name in ['main.rs','fixtures.rs','handwritten.rs','metrics.rs','typed.rs','validation.rs','simd.rs']:
    if (root/'benchmarks/absolute'/name).exists():shutil.copy2(root/'benchmarks/absolute'/name,build/'src'/name)
for module in ['reference','foundation']:
    test=build/'src'/module/'jsonata/tests.rs'
    test.write_text(test.read_text().replace('crate::transform::build_plan',f'crate::{module}::build_plan'))
(build/'src/simd_metrics.rs').write_text((root/'benchmarks/absolute/metrics.rs').read_text()+'\npub fn live_bytes()->i64 { LIVE.load(Relaxed) }\n')
# The independently built crate keeps benchmark dependencies out of production Cargo manifests.
(build/'Cargo.toml').write_text(f'''[package]
name="jx-absolute"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
jx={{path={json.dumps(str(root/'crates/jx'))},features=["experimental"]}}
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
if (root/'benchmarks/absolute/Cargo.lock').exists():shutil.copy2(root/'benchmarks/absolute/Cargo.lock',build/'Cargo.lock')
run(['cargo','build','--manifest-path',str(build/'Cargo.toml'),'--release','--offline','--locked']+(['--bin','simd-reference'] if a.simd_only else []))
identity={str(f.relative_to(root)):hashlib.sha256(f.read_bytes()).hexdigest() for f in (root/'benchmarks/absolute').glob('*') if f.is_file()}
(out/('simd-build.json' if a.simd_only else 'build.json')).write_text(json.dumps({'head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'files':identity,'binary_sha256':hashlib.sha256((build/('target/release/simd-reference' if a.simd_only else 'target/release/jx-absolute')).read_bytes()).hexdigest()},indent=2)+'\n')
print(build/('target/release/simd-reference' if a.simd_only else 'target/release/jx-absolute'))
