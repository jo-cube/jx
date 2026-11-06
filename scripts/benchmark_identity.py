"""Artifact and source provenance shared by the worker benchmark builders."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys


def hashes(root, paths):
    return {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(set(paths)) if path.is_file()}


def source_identity(manifest):
    manifest = Path(manifest).resolve()
    root = manifest.parent
    try:
        revision = subprocess.check_output(
            ['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True, stderr=subprocess.DEVNULL).strip()
        dirty = bool(subprocess.check_output(
            ['git', '--no-optional-locks', '-C', str(root), 'status', '--porcelain'],
            stderr=subprocess.DEVNULL))
    except subprocess.CalledProcessError:
        revision = dirty = None
    return dict(manifest_path=str(manifest), revision=revision, dirty=dirty,
                source_sha256=hashes(root, [manifest, root / 'build.rs', *(root / 'src').rglob('*')]))


def cargo_artifact(command, cwd, name):
    command = [*command, '--message-format=json']
    result = subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE, text=True)
    artifacts = []
    for line in result.stdout.splitlines():
        message = json.loads(line)
        if message.get('reason') == 'compiler-message':
            rendered = message['message'].get('rendered')
            if rendered:
                print(rendered, file=sys.stderr, end='')
        elif message.get('reason') == 'compiler-artifact':
            artifacts.append(message)
    result.check_returncode()
    selected = [item for item in artifacts if item['target']['name'] == name]
    if len(selected) != 1:
        raise ValueError(f'expected one Cargo artifact for {name}, found {len(selected)}')
    engine = next(item for item in artifacts
                  if item['target']['name'] == 'jx' and 'lib' in item['target']['kind'])
    manifest = Path(command[command.index('--manifest-path') + 1]) if '--manifest-path' in command else cwd / 'Cargo.toml'
    record = dict(command=command, manifest_path=str(manifest),
                  manifest_sha256=hashlib.sha256(manifest.read_bytes()).hexdigest(),
                  lockfile_sha256=hashlib.sha256(manifest.with_name('Cargo.lock').read_bytes()).hexdigest(),
                  engine=dict(source_identity(engine['manifest_path']), features=engine['features']),
                  artifacts=[{key: item[key] for key in
                              ('package_id', 'manifest_path', 'target', 'profile', 'features', 'filenames', 'executable')}
                             for item in artifacts])
    return selected[0], record
