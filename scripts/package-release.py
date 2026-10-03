#!/usr/bin/env python3
"""Archive an already-built CLI variant; never builds, uploads or publishes."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile
import time
import zipfile

ROOT = Path(__file__).resolve().parent.parent
TARGETS = (
    'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    'x86_64-apple-darwin', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc',
)


def archive(binary, target, version, revision, epoch, output, root=ROOT, variant="default"):
    if variant not in ('default', 'native'):
        raise ValueError('unsupported release variant')
    if target not in TARGETS or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?', version):
        raise ValueError('unsupported target or invalid version')
    name = f'jx-v{version}-{target}' + ('-native' if variant == 'native' else '')
    notices = 'THIRD_PARTY_LICENSES_NATIVE' if variant == 'native' else 'THIRD_PARTY_LICENSES'
    features = 'jit (execution requires --jit)' if variant == 'native' else 'default (no native backend)'
    files = {
        'jx.exe' if target.endswith('msvc') else 'jx': binary.read_bytes(),
        'README.md': (root / 'crates/jx-cli/README.md').read_bytes(),
        'LICENSE': (root / 'LICENSE').read_bytes(),
        'THIRD_PARTY_LICENSES': (root / notices).read_bytes(),
        'CLI.md': (root / 'docs/cli.md').read_bytes(),
        'COMPATIBILITY.md': (root / 'docs/compatibility.md').read_bytes(),
        'BUILD.txt': f'version={version}\nrevision={revision}\ntarget={target}\nrust=1.98.1\nvariant={variant}\nfeatures={features}\nsource_date_epoch={epoch}\n'.encode(),
    }
    output.mkdir(parents=True, exist_ok=True)
    path = output / (name + ('.zip' if target.endswith('msvc') else '.tar.gz'))
    if target.endswith('msvc'):
        # ZIP has a two-second timestamp precision and a 1980 lower bound.
        stamp = time.gmtime(max(epoch, 315532800))[:6]
        with zipfile.ZipFile(path, 'w', compression=zipfile.ZIP_DEFLATED) as packed:
            for member, data in sorted(files.items()):
                info = zipfile.ZipInfo(f'{name}/{member}', stamp)
                info.compress_type = zipfile.ZIP_DEFLATED
                info.create_system = 3
                info.external_attr = (0o100755 if member == 'jx.exe' else 0o100644) << 16
                packed.writestr(info, data)
    else:
        with path.open('wb') as sink, gzip.GzipFile(filename='', mode='wb', fileobj=sink, mtime=epoch) as compressed:
            with tarfile.open(fileobj=compressed, mode='w', format=tarfile.USTAR_FORMAT) as packed:
                for member, data in sorted(files.items()):
                    info = tarfile.TarInfo(f'{name}/{member}')
                    info.size = len(data)
                    info.mtime = epoch
                    info.mode = 0o755 if member == 'jx' else 0o644
                    packed.addfile(info, io.BytesIO(data))
    checksum = hashlib.sha256(path.read_bytes()).hexdigest()
    path.with_name(path.name + '.sha256').write_text(f'{checksum}  {path.name}\n', encoding='ascii')
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True, choices=TARGETS)
    parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    parser.add_argument('--variant', choices=('default', 'native'), default='default')
    args = parser.parse_args()
    status = subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)
    if status:
        parser.error('release packaging requires a clean tree')
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], cwd=ROOT))
    version = next(p['version'] for p in metadata['packages'] if p['name'] == 'jx-cli')
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    epoch = int(subprocess.check_output(['git', 'show', '-s', '--format=%ct', 'HEAD'], cwd=ROOT))
    executable = 'jx.exe' if args.target.endswith('msvc') else 'jx'
    binary = ROOT / 'target' / args.target / 'release' / executable
    # Running the binary also catches accidental cross-target packaging or stale builds.
    actual = subprocess.check_output([binary, '--version'], text=True).strip()
    if actual != f'jx {version}':
        parser.error(f'binary version mismatch: {actual}')
    help_text = subprocess.check_output([binary, '--help'], text=True)
    if ('--jit' in help_text) != (args.variant == 'native'):
        parser.error('binary native support does not match the requested release variant')
    print(archive(binary, args.target, version, revision, epoch, args.output, variant=args.variant))


if __name__ == '__main__':
    main()
