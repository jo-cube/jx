#!/usr/bin/env python3
"""Verify a release archive, then exercise only its extracted executable."""
import argparse
import hashlib
from pathlib import Path
import subprocess
import tarfile
import tempfile
import zipfile


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inspect_archive(path, revision=None):
    checksum = hashlib.sha256(path.read_bytes()).hexdigest()
    require(path.with_name(path.name + '.sha256').read_text(encoding='ascii') ==
            f'{checksum}  {path.name}\n', 'checksum mismatch')
    if path.suffix == '.zip':
        with zipfile.ZipFile(path) as packed:
            names = packed.namelist()
            files = {name: packed.read(name) for name in names}
    else:
        with tarfile.open(path) as packed:
            members = packed.getmembers()
            require(all(member.isfile() for member in members), 'archive must contain regular files')
            names = [member.name for member in members]
            files = {member.name: packed.extractfile(member).read() for member in members}
    require(len(files) == len(names), 'duplicate archive member')
    builds = [name for name in names if name.endswith('/BUILD.txt')]
    require(len(builds) == 1, 'missing or duplicate BUILD.txt')
    lines = files[builds[0]].decode('utf-8').splitlines()
    build = dict(line.split('=', 1) for line in lines)
    require(len(build) == len(lines) and set(build) == {
        'version', 'revision', 'target', 'rust', 'variant', 'features', 'source_date_epoch',
    }, 'invalid build metadata')
    require(build['variant'] in ('default', 'native'), 'unknown build variant')
    native = build['variant'] == 'native'
    features = 'jit (execution requires --jit)' if native else 'default (no native backend)'
    require(build['features'] == features, 'inconsistent feature metadata')
    if revision is not None:
        require(build['revision'] == revision, 'unexpected source revision')
    name = f"jx-v{build['version']}-{build['target']}" + ('-native' if native else '')
    windows = build['target'].endswith('msvc')
    require(path.name == name + ('.zip' if windows else '.tar.gz'), 'unexpected archive name')
    executable = 'jx.exe' if windows else 'jx'
    required = {executable, 'README.md', 'LICENSE', 'CLI.md', 'COMPATIBILITY.md',
                'BUILD.txt', 'THIRD_PARTY_LICENSES'}
    require(set(files) == {f'{name}/{member}' for member in required}, 'unexpected archive contents')
    return build, {member: files[f'{name}/{member}'] for member in required}


def smoke(path, revision=None):
    build, files = inspect_archive(path, revision)
    native = build['variant'] == 'native'
    with tempfile.TemporaryDirectory(prefix='jx-release-') as temporary:
        root = Path(temporary)
        for name, data in files.items():
            (root / name).write_bytes(data)
        executable = root / ('jx.exe' if build['target'].endswith('msvc') else 'jx')
        executable.chmod(0o755)

        def run(args, data=b'', status=0, output=None, error=None):
            child = subprocess.run([str(executable), *map(str, args)], input=data,
                                   capture_output=True, timeout=30)
            require(child.returncode == status,
                    f'{args}: exit {child.returncode}, expected {status}: {child.stderr!r}')
            if output is not None:
                require(child.stdout == output, f'{args}: unexpected stdout {child.stdout!r}')
            if status == 0:
                require(not child.stderr, f'{args}: unexpected stderr {child.stderr!r}')
            if error is not None:
                require(error in child.stderr, f'{args}: missing diagnostic {error!r}')
            return child.stdout

        version = run(['--version']).decode('utf-8').strip()
        require(version == f"jx {build['version']}", 'binary version mismatch')
        help_text = run(['--help'])
        require((b'--jit' in help_text) == native, 'binary native capability mismatch')
        run(['a'], b'\r\n{}\n{"a":null}\r\n{"a":[1,2]}\n{"a":3}',
            output=b'null\n[1,2]\n3\n')
        run(['a.b'], b'{"a":[{"b":1},{"b":2}]}\n', output=b'1\n2\n')
        records = root / 'records with spaces.ndjson'
        records.write_bytes(b'{"a":1}\r\n{"a":2}')
        run(['a', records, '-', records], b'{"a":3}\n', output=b'1\n2\n3\n1\n2\n')
        expression = root / 'expression.jsonata'
        expression.write_text('/* example */\n{"total":$sum(items.(price*quantity))}', encoding='utf-8')
        data = b'{"items":[{"price":2.5,"quantity":3}]}\n'
        run(['-f', expression], data, output=b'{"total":7.5}\n')
        run(['--not-an-option'], status=2, output=b'')
        run(['a['], status=2, output=b'', error=b'Compilation')
        run(['-f', root / 'absent.jsonata'], status=2, output=b'')
        run(['$'], b'1\n[0,]\n2\n', status=1, output=b'1\n', error=b'line 2')
        run(['a+1'], b'{"a":null}\n', status=1, output=b'', error=b'TypeError')
        run(['{"ok":1,"fn":function(){1}}'], b'{}\n', status=1,
            output=b'', error=b'serialization')
        run(['--max-record-bytes', '4', '$'], b'12345\n', status=1,
            output=b'', error=b'max-record-bytes')
        run(['--max-output-bytes', '4', '$'], b'12345\n', status=1,
            output=b'', error=b'max-output-bytes')
        run(['--max-work', '1', '$map([1..20],function($n){$n+1})'], b'{}\n',
            status=1, output=b'', error=b'EvaluationLimit')
        if native:
            run(['--jit', 'price*quantity'], b'{"price":2.5,"quantity":3}\n', output=b'7.5\n')
            run(['--jit', '$sum(rows[$>2].($*$))'], b'{"rows":[1,2,3,4]}\n', output=b'25\n')
            run(['--jit', 'a+1'], b'{"a":null}\n', status=1, output=b'', error=b'TypeError')
        else:
            run(['--jit', '$'], status=2, output=b'')
    print(f'{path.name}: checksum, contents and extracted CLI smoke passed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--revision', help='expected source commit recorded in BUILD.txt')
    args = parser.parse_args()
    smoke(args.archive, args.revision)


if __name__ == '__main__':
    main()
