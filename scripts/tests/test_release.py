import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location('release', Path(__file__).parents[1] / 'package-release.py')
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseArchive(unittest.TestCase):
    def test_contents_checksums_and_reproducibility(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / 'binary'
            binary.write_bytes(b'fixed binary bytes\x00')
            for target in release.TARGETS:
                for variant in ('default', 'native'):
                    with self.subTest(target=target, variant=variant):
                        first = release.archive(binary, target, '0.1.0', 'abc', 1750000000,
                                                root / 'a', variant=variant)
                        second = release.archive(binary, target, '0.1.0', 'abc', 1750000000,
                                                 root / 'b', variant=variant)
                        name = f'jx-v0.1.0-{target}' + ('-native' if variant == 'native' else '')
                        suffix = '.zip' if target.endswith('msvc') else '.tar.gz'
                        self.assertEqual(first.name, name + suffix)
                        self.assertEqual(first.read_bytes(), second.read_bytes())
                        digest = hashlib.sha256(first.read_bytes()).hexdigest()
                        self.assertEqual(first.with_name(first.name + '.sha256').read_text(),
                                         f'{digest}  {first.name}\n')
                        if first.suffix == '.zip':
                            with zipfile.ZipFile(first) as packed:
                                contents = {p.split('/')[-1]: packed.read(p) for p in packed.namelist()}
                                self.assertTrue(all(p.startswith(name + '/') for p in packed.namelist()))
                        else:
                            with tarfile.open(first) as packed:
                                contents = {p.name.split('/')[-1]: packed.extractfile(p).read()
                                            for p in packed.getmembers()}
                                self.assertEqual(packed.getmembers()[0].uid, 0)
                                self.assertTrue(all(p.name.startswith(name + '/') for p in packed.getmembers()))
                                executable = next(p for p in packed.getmembers() if p.name.endswith('/jx'))
                                self.assertEqual(executable.mode, 0o755)
                        exe = 'jx.exe' if target.endswith('msvc') else 'jx'
                        self.assertEqual(contents[exe], binary.read_bytes())
                        self.assertEqual(set(contents), {exe, 'README.md', 'LICENSE', 'CLI.md',
                                                        'COMPATIBILITY.md', 'BUILD.txt', 'THIRD_PARTY_LICENSES'})
                        features = 'jit (execution requires --jit)' if variant == 'native' else 'default (no native backend)'
                        self.assertIn(f'variant={variant}'.encode(), contents['BUILD.txt'])
                        self.assertIn(f'features={features}'.encode(), contents['BUILD.txt'])
                        notices = 'THIRD_PARTY_LICENSES_NATIVE' if variant == 'native' else 'THIRD_PARTY_LICENSES'
                        self.assertEqual(contents['THIRD_PARTY_LICENSES'], (release.ROOT / notices).read_bytes())

    def test_package_licenses_match_root(self):
        for name in ['jx', 'jx-cli', 'jx-native']:
            self.assertEqual((release.ROOT / f'crates/{name}/LICENSE').read_bytes(), (release.ROOT / 'LICENSE').read_bytes())

    def test_rejects_unsafe_names(self):
        with self.assertRaises(ValueError):
            release.archive(Path('missing'), 'x86_64-apple-darwin', '../bad', 'abc', 1, Path('.'))
        with self.assertRaises(ValueError):
            release.archive(Path('missing'), 'unknown', '0.1.0', 'abc', 1, Path('.'))
        with self.assertRaises(ValueError):
            release.archive(Path('missing'), 'x86_64-apple-darwin', '0.1.0', 'abc', 1, Path('.'), variant='unknown')


if __name__ == '__main__':
    unittest.main()
