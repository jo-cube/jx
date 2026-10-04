#!/usr/bin/env python3
"""Refresh checked-in notices for the locked native release dependency graph."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def main():
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--format-version', '1', '--locked', '--features', 'jx-cli/jit',
    ], cwd=ROOT))
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    pending = [p['id'] for p in packages.values() if p['name'] == 'jx-cli']
    seen = set()
    while pending:
        item = pending.pop()
        if item in seen:
            continue
        seen.add(item)
        pending.extend(d['pkg'] for d in nodes[item]['deps']
                       if any(k['kind'] in (None, 'build') for k in d['dep_kinds']))
    dependencies = sorted((packages[i] for i in seen if packages[i]['source']),
                          key=lambda p: (p['name'], p['version']))
    llvm = next(Path(p['manifest_path']).parent / 'LICENSE'
                for p in dependencies if p['name'] == 'cranelift-codegen').read_text(encoding="utf-8")
    assert 'LLVM Exceptions' in llvm
    groups = {}
    for package in dependencies:
        directory = Path(package['manifest_path']).parent
        files = {p.name.lower(): p for p in directory.iterdir() if p.is_file()}
        license_id = package['license']
        if 'MIT' in license_id:
            path = next(files[n] for n in ('license-mit', 'license.txt', 'license') if n in files)
            texts = [path.read_text(encoding="utf-8")]
        elif license_id == 'Apache-2.0 WITH LLVM-exception':
            texts = [files['license'].read_text(encoding="utf-8") if 'license' in files else llvm]
        elif license_id == 'Zlib':
            texts = [files['license'].read_text(encoding="utf-8")]
        else:
            raise ValueError(f"review license for {package['name']}: {license_id}")
        if 'Unicode-3.0' in license_id:
            texts.append(files['license-unicode'].read_text(encoding="utf-8"))
        texts.extend(p.read_text(encoding="utf-8") for name, p in files.items() if name.startswith('notice'))
        for text in texts:
            groups.setdefault(text, []).append(f"{package['name']} {package['version']}")
    sections = [
        'Third-party notices for native-capable jx CLI binaries\n\n'
        'Locked normal/build dependencies across supported targets are included.\n'
        'Dual-licensed MIT dependencies use their MIT option. Original license texts follow.\n',
    ]
    for text, names in groups.items():
        sections.append('--- ' + ', '.join(names) + ' ---\n\n' + text.rstrip() + '\n')
    standard = (ROOT / 'THIRD_PARTY_LICENSES').read_text(encoding="utf-8").split('--- Rust standard library', 1)[1]
    sections.append('--- Rust standard library' + standard)
    (ROOT / 'THIRD_PARTY_LICENSES_NATIVE').write_text('\n\n'.join(sections), encoding='utf-8')


if __name__ == '__main__':
    main()
