import hashlib
import importlib.util
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('package', ROOT / 'scripts/package-release.py')
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)
BASE = 'https://github.com/jo-cube/jx'
BINARY = b'#!/bin/sh\nprintf "jx fixture\\n"\n'


@unittest.skipUnless(os.name == 'posix', 'the installer supports Unix only')
class Installer(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.bin = self.root / 'tools'
        self.fixtures = self.root / 'fixtures'
        self.downloads = self.root / 'downloads'
        self.destination = self.root / 'install with spaces'
        for path in [self.bin, self.fixtures, self.downloads, self.destination]:
            path.mkdir()
        self.old = self.destination / 'jx'
        self.old.write_bytes(b'previous executable')
        self.log = self.root / 'requests'
        for tool in ['awk', 'cat', 'chmod', 'mkdir', 'mktemp', 'mv', 'rm', 'tar', 'gzip',
                     'sha256sum', 'shasum']:
            actual = shutil.which(tool)
            if actual:
                (self.bin / tool).symlink_to(actual)
        (self.bin / 'python3').symlink_to(sys.executable)
        self.stub('uname', '#!/bin/sh\ncase "$1" in -s) echo "$TEST_OS";; -m) echo "$TEST_ARCH";; esac\n')
        self.stub('curl', f'#!{sys.executable}\n' + '''import os
from pathlib import Path
import shutil
import sys
args = sys.argv[1:]
url = next(value for value in args if value.startswith('https://'))
with open(os.environ['TEST_LOG'], 'a') as log:
    log.write(url + '\\n')
kind = 'latest' if url.endswith('/latest') else ('checksum' if url.endswith('.sha256') else 'archive')
if os.environ.get('TEST_FAIL') == kind:
    print('curl: simulated HTTP failure', file=sys.stderr)
    sys.exit(22)
if kind == 'latest':
    print(os.environ['TEST_LATEST'], end='')
else:
    source = Path(os.environ['TEST_FIXTURES']) / url.rsplit('/', 1)[1]
    if not source.is_file():
        sys.exit(22)
    shutil.copyfile(source, args[args.index('-o') + 1])
    if kind == 'checksum' and os.environ.get('TEST_TARGET_DIRECTORY'):
        target = Path(os.environ['TEST_TARGET_DIRECTORY'])
        target.unlink()
        target.mkdir()
''')
        self.env = dict(os.environ, PATH=str(self.bin), HOME=str(self.root / 'home'),
                        TMPDIR=str(self.downloads), TEST_OS='Linux', TEST_ARCH='x86_64',
                        TEST_LOG=str(self.log), TEST_FIXTURES=str(self.fixtures),
                        TEST_LATEST=BASE + '/releases/tag/v0.2.0')
        self.archive()

    def stub(self, name, source):
        path = self.bin / name
        if path.exists() or path.is_symlink():
            path.unlink()
        path.write_text(source)
        path.chmod(0o755)

    def archive(self, target='x86_64-unknown-linux-gnu', binary=BINARY):
        executable = self.root / 'binary'
        executable.write_bytes(binary)
        return package.archive(executable, target, '0.2.0', 'a' * 40, 1750000000, self.fixtures)

    def checksum(self, path):
        path.with_name(path.name + '.sha256').write_text(
            f'{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n')

    def invoke(self, arguments=None, version='v0.2.0', **settings):
        env = dict(self.env, **settings)
        if version is None:
            env.pop('VERSION', None)
        else:
            env['VERSION'] = version
        if self.log.exists():
            self.log.unlink()
        result = subprocess.run(['/bin/sh', str(ROOT / 'scripts/install.sh'),
                                 *([str(self.destination)] if arguments is None else arguments)],
                                env=env, capture_output=True, timeout=10)
        self.assertEqual(list(self.downloads.iterdir()), [], result.stderr)
        self.assertEqual(list(self.root.rglob('.jx-install.*')), [], result.stderr)
        requests = self.log.read_text().splitlines() if self.log.exists() else []
        return result, requests

    def test_explicit_version_platform_mapping_and_atomic_replacement(self):
        for system, arch, target in [
            ('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
            ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu'),
            ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
            ('Darwin', 'arm64', 'aarch64-apple-darwin'),
        ]:
            with self.subTest(system=system, arch=arch):
                archive = self.archive(target)
                result, requests = self.invoke(TEST_OS=system, TEST_ARCH=arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                url = f'{BASE}/releases/download/v0.2.0/{archive.name}'
                self.assertEqual(requests, [url, url + '.sha256'])
                self.assertEqual(self.old.read_bytes(), BINARY)
                self.assertEqual(self.old.stat().st_mode & 0o777, 0o755)
                self.assertEqual(subprocess.check_output([self.old]), b'jx fixture\n')

    def test_latest_is_resolved_once_and_default_directory_is_user_local(self):
        result, requests = self.invoke(arguments=[], version=None)
        self.assertEqual(result.returncode, 0, result.stderr)
        name = 'jx-v0.2.0-x86_64-unknown-linux-gnu.tar.gz'
        url = f'{BASE}/releases/download/v0.2.0/{name}'
        self.assertEqual(requests, [BASE + '/releases/latest', url, url + '.sha256'])
        self.assertEqual((self.root / 'home/.local/bin/jx').read_bytes(), BINARY)

    def test_help_invalid_arguments_versions_and_unsupported_platforms_are_offline(self):
        result, requests = self.invoke(arguments=['--help'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(b'VERSION', result.stdout)
        self.assertEqual(requests, [])
        for arguments, version, settings in [
            (['one', 'two'], 'v0.2.0', {}),
            ([''], 'v0.2.0', {}),
            (['--unknown'], 'v0.2.0', {}),
            (None, 'v01.2.0', {}),
            (None, 'v0.2.0-rc.1', {}),
            (None, '../escape', {}),
            (None, 'v0.2.0', {'TEST_OS': 'FreeBSD'}),
            (None, 'v0.2.0', {'TEST_ARCH': 'i686'}),
        ]:
            with self.subTest(arguments=arguments, version=version, settings=settings):
                result, requests = self.invoke(arguments, version, **settings)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stderr)
                self.assertEqual(requests, [])
                self.assertEqual(self.old.read_bytes(), b'previous executable')

    def test_download_and_missing_release_failures_preserve_existing_installation(self):
        for failure in ['latest', 'archive', 'checksum']:
            with self.subTest(failure=failure):
                result, _ = self.invoke(version=None, TEST_FAIL=failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b'download' if failure != 'latest' else b'latest', result.stderr)
                self.assertEqual(self.old.read_bytes(), b'previous executable')
        result, _ = self.invoke(version=None, TEST_LATEST=BASE + '/releases/tag/not-a-version')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'latest', result.stderr)

    def test_checksum_mismatch_and_untrusted_checksum_names_are_rejected(self):
        archive = self.archive()
        for content in ['0' * 64 + f'  {archive.name}\n',
                        '0' * 64 + '  ../../outside\n',
                        'not a checksum\n']:
            with self.subTest(content=content):
                archive.with_name(archive.name + '.sha256').write_text(content)
                result, _ = self.invoke()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b'checksum', result.stderr)
                self.assertEqual(self.old.read_bytes(), b'previous executable')

    def test_archive_layout_and_nonregular_members_are_rejected(self):
        for kind in ['absolute', 'traversal', 'wrong-root', 'extra', 'duplicate', 'missing',
                     'symlink', 'hardlink', 'directory', 'truncated']:
            with self.subTest(kind=kind):
                archive = self.archive()
                with tarfile.open(archive) as source:
                    members = [(member, source.extractfile(member).read()) for member in source.getmembers()]
                first, data = members[0]
                if kind == 'absolute':
                    first.name = '/' + first.name
                elif kind == 'traversal':
                    first.name = '../' + first.name
                elif kind == 'wrong-root':
                    first.name = 'other/' + first.name.split('/', 1)[1]
                elif kind == 'extra':
                    members.append((tarfile.TarInfo('unexpected'), b''))
                elif kind == 'duplicate':
                    members.append(members[0])
                elif kind == 'missing':
                    members.pop()
                elif kind in ['symlink', 'hardlink', 'directory']:
                    first.type = {'symlink': tarfile.SYMTYPE, 'hardlink': tarfile.LNKTYPE,
                                  'directory': tarfile.DIRTYPE}[kind]
                    first.linkname = '/outside'
                    first.size = 0
                with tarfile.open(archive, 'w:gz', format=tarfile.USTAR_FORMAT) as output:
                    for member, data in members:
                        output.addfile(member, io.BytesIO(data) if member.isfile() else None)
                if kind == 'truncated':
                    archive.write_bytes(archive.read_bytes()[:32])
                self.checksum(archive)
                result, _ = self.invoke()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b'archive', result.stderr)
                self.assertEqual(self.old.read_bytes(), b'previous executable')

    def test_empty_executable_and_failed_rename_leave_previous_binary_intact(self):
        self.archive(binary=b'')
        result, _ = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'empty', result.stderr)
        self.assertEqual(self.old.read_bytes(), b'previous executable')
        self.archive()
        self.stub('mv', '#!/bin/sh\nexit 1\n')
        result, _ = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'install', result.stderr)
        self.assertEqual(self.old.read_bytes(), b'previous executable')

    def test_target_becoming_a_directory_does_not_receive_the_executable(self):
        result, _ = self.invoke(TEST_TARGET_DIRECTORY=str(self.old))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'install', result.stderr)
        self.assertTrue(self.old.is_dir())
        self.assertEqual(list(self.old.iterdir()), [])

    def test_installation_replaces_a_file_symlink_without_touching_its_target(self):
        target = self.root / 'outside'
        target.write_bytes(b'unchanged target')
        self.old.unlink()
        self.old.symlink_to(target)
        result, _ = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.old.is_symlink())
        self.assertEqual(self.old.read_bytes(), BINARY)
        self.assertEqual(target.read_bytes(), b'unchanged target')

    def test_checksum_tool_fallback_and_missing_dependencies(self):
        for hasher in ['sha256sum', 'shasum']:
            if (self.bin / hasher).exists():
                alternative = self.bin / ('shasum' if hasher == 'sha256sum' else 'sha256sum')
                saved = alternative.readlink() if alternative.is_symlink() else None
                if saved:
                    alternative.unlink()
                result, _ = self.invoke()
                self.assertEqual(result.returncode, 0, result.stderr)
                if saved:
                    alternative.symlink_to(saved)
        for tool in ['sha256sum', 'shasum']:
            path = self.bin / tool
            if path.is_symlink():
                path.unlink()
        result, requests = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'SHA-256', result.stderr)
        self.assertEqual(requests, [])
        (self.bin / 'curl').unlink()
        result, requests = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'curl', result.stderr)
        self.assertEqual(requests, [])

    @unittest.skipIf(hasattr(os, 'geteuid') and os.geteuid() == 0, 'requires unprivileged permissions')
    def test_unwritable_destination_reports_permission_failure(self):
        self.destination.chmod(0o555)
        self.addCleanup(self.destination.chmod, 0o755)
        result, _ = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'install', result.stderr)
        self.assertEqual(self.old.read_bytes(), b'previous executable')


if __name__ == '__main__':
    unittest.main()
