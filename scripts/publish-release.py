#!/usr/bin/env python3
"""Validate an existing release tag and publish only complete, verified archives."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.parse import urlencode
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location('package', ROOT / 'scripts/package-release.py')
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)
SPEC = importlib.util.spec_from_file_location('smoke', ROOT / 'scripts/smoke-release.py')
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)
require = smoke.require


def validate_version(root, tag):
    require(re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag),
            'release tag must be vX.Y.Z without leading zeroes')
    version = tag[1:]
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    require(workspace['package']['version'] == version, 'tag/workspace version mismatch')
    locked = tomllib.loads((root / 'Cargo.lock').read_text())['package']
    for member in workspace['members']:
        manifest = tomllib.loads((root / member / 'Cargo.toml').read_text())['package']
        actual = manifest['version']
        if actual == {'workspace': True}:
            actual = workspace['package']['version']
        name = manifest['name']
        require(actual == version, f'{name} package version mismatch')
        require([entry['version'] for entry in locked
                 if entry['name'] == name and 'source' not in entry] == [version],
                f'{name} lockfile version mismatch')
    cli = tomllib.loads((root / 'crates/jx-cli/Cargo.toml').read_text())
    dependency = cli['dependencies']['jx']
    require(dependency['version'] == f'={version}', 'jx-cli internal jx dependency version mismatch')
    require((root / 'crates/jx-cli' / dependency['path']).resolve() == (root / 'crates/jx').resolve(),
            'jx-cli internal jx dependency path mismatch')
    return version


def git(root, *args):
    return subprocess.check_output(['git', *args], cwd=root, text=True).strip()


def check_tag(root, tag, revision):
    version = validate_version(root, tag)
    require(re.fullmatch('[0-9a-f]{40}', revision), 'expected a full Git revision')
    commit = git(root, 'rev-parse', '--verify', revision + '^{commit}')
    require(git(root, 'rev-parse', 'HEAD') == commit, 'checkout/trigger revision mismatch')
    require(git(root, 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}') == commit,
            'tag/trigger revision mismatch')
    return version, commit


def check_artifacts(directory, version, revision):
    archives = [directory / f'jx-v{version}-{target}{".zip" if target.endswith("msvc") else ".tar.gz"}'
                for target in package.TARGETS]
    files = archives + [path.with_name(path.name + '.sha256') for path in archives]
    require({path.name for path in directory.iterdir()} == {path.name for path in files}
            and all(path.is_file() and not path.is_symlink() for path in files),
            'release artifacts must contain exactly five archives and their checksums')
    for path, target in zip(archives, package.TARGETS):
        build, _ = smoke.inspect_archive(path, revision)
        require(build['version'] == version and build['target'] == target,
                'archive release identity mismatch')
    return sorted(files)


def check_remote_tag(repository, tag, revision):
    ref = f'refs/tags/{tag}'
    output = subprocess.check_output(['git', 'ls-remote', '--exit-code',
                                      f'https://github.com/{repository}.git', ref, ref + '^{}'], text=True)
    refs = {name: commit for commit, name in (line.split() for line in output.splitlines())}
    require(refs.get(ref + '^{}', refs.get(ref)) == revision, 'remote tag revision changed or is missing')


def api(method, url, data=None, content_type='application/json'):
    token = os.environ.get('GH_TOKEN')
    require(token, 'GH_TOKEN is required for publication')
    if isinstance(data, dict):
        data = json.dumps(data).encode('utf-8')
    request = Request(url, data=data, method=method, headers={
        'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json',
        'Content-Type': content_type, 'X-GitHub-Api-Version': '2022-11-28',
    })
    with urlopen(request, timeout=60) as response:
        return json.load(response)


def publish(root, tag, revision, directory, repository):
    version, revision = check_tag(root, tag, revision)
    files = check_artifacts(directory, version, revision)
    # Pin validated identities before network requests; later file changes must not redefine them.
    expected = {}
    for path in files:
        data = path.read_bytes()
        expected[path.name] = (len(data), 'sha256:' + hashlib.sha256(data).hexdigest())
    check_remote_tag(repository, tag, revision)
    base = f'https://api.github.com/repos/{repository}/releases'
    # Listing with the publisher's token includes drafts, unlike the published-tag endpoint.
    page = 1
    while True:
        releases = api('GET', f'{base}?per_page=100&page={page}')
        require(not any(release['tag_name'] == tag for release in releases),
                f'release {tag} already exists; inspect it before retrying')
        if len(releases) < 100:
            break
        page += 1
    draft = api('POST', base, {'tag_name': tag, 'target_commitish': revision, 'name': tag,
                              'body': f'Source revision: `{revision}`.', 'draft': True})
    require(draft['draft'] and draft['tag_name'] == tag, 'unexpected draft release identity')
    release_url = f"{base}/{draft['id']}"
    for path in files:
        upload = f"https://uploads.github.com/repos/{repository}/releases/{draft['id']}/assets?"
        data = path.read_bytes()
        require((len(data), 'sha256:' + hashlib.sha256(data).hexdigest()) == expected[path.name],
                f'validated artifact changed: {path.name}')
        api('POST', upload + urlencode({'name': path.name}), data, 'application/octet-stream')
    draft = api('GET', release_url)
    require(draft['draft'] and draft['tag_name'] == tag,
            'draft release identity changed')
    assets = draft['assets']
    require(len(assets) == len(files) and {asset['name'] for asset in assets} == {path.name for path in files},
            'uploaded release artifacts are incomplete or unexpected')
    for asset in assets:
        require(asset['state'] == 'uploaded'
                and (asset['size'], asset.get('digest')) == expected[asset['name']],
                f'uploaded asset verification failed: {asset["name"]}')
    check_remote_tag(repository, tag, revision)
    result = api('PATCH', release_url, {'draft': False, 'make_latest': 'legacy'})
    require(not result['draft'] and result['tag_name'] == tag, 'release publication was not confirmed')
    return f'https://github.com/{repository}/releases/tag/{tag}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    check = commands.add_parser('check-tag')
    release = commands.add_parser('publish')
    for command in (check, release):
        command.add_argument('--tag', required=True)
        command.add_argument('--revision', required=True)
    release.add_argument('--artifacts', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'check-tag':
            check_tag(ROOT, args.tag, args.revision)
        else:
            repository = os.environ.get('GITHUB_REPOSITORY')
            require(repository, 'GITHUB_REPOSITORY is required for publication')
            print(publish(ROOT, args.tag, args.revision, args.artifacts, repository))
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'release: {error}\nInspect any release/draft before retrying.\n')


if __name__ == '__main__':
    main()
