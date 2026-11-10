import copy
import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlparse

SPEC = importlib.util.spec_from_file_location(
    'publication', Path(__file__).parents[1] / 'publish-release.py')
publication = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publication)
REVISION = 'a' * 40
TAG = 'v0.2.0'


class Publication(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        fixtures = {
            'Cargo.toml': '[workspace]\nmembers=["crates/jx","crates/jx-cli"]\n'
                          '[workspace.package]\nversion = "0.2.0"\n',
            'Cargo.lock': '[[package]]\nname="jx"\nversion = "0.2.0"\n'
                          '[[package]]\nname="jx-cli"\nversion = "0.2.0"\n',
            'crates/jx/Cargo.toml': '[package]\nname="jx"\nversion.workspace = true\n',
            'crates/jx-cli/Cargo.toml': '[package]\nname="jx-cli"\nversion.workspace = true\n'
                                      '[dependencies]\njx={version = "=0.2.0",path="../jx"}\n',
        }
        for name, contents in fixtures.items():
            target = self.root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(contents)
        self.artifacts = self.root / 'dist'
        binary = self.root / 'binary'
        binary.write_bytes(b'fixture executable')
        for target in publication.package.TARGETS:
            publication.package.archive(binary, target, '0.2.0', REVISION, 1750000000, self.artifacts)

    def test_version_checks_workspace_members_lockfile_and_internal_dependency(self):
        self.assertEqual(publication.validate_version(self.root, TAG), '0.2.0')
        for name, old, new in [
            ('Cargo.toml', 'version = "0.2.0"', 'version = "0.3.0"'),
            ('crates/jx/Cargo.toml', 'version.workspace = true', 'version = "0.3.0"'),
            ('crates/jx-cli/Cargo.toml', 'version.workspace = true', 'version = "0.3.0"'),
            ('Cargo.lock', 'name="jx"\nversion = "0.2.0"', 'name="jx"\nversion = "0.3.0"'),
            ('Cargo.lock', 'name="jx-cli"\nversion = "0.2.0"', 'name="jx-cli"\nversion = "0.3.0"'),
            ('crates/jx-cli/Cargo.toml', 'version = "=0.2.0"', 'version = "=0.1.0"'),
        ]:
            with self.subTest(name=name, old=old):
                path = self.root / name
                original = path.read_text()
                path.write_text(original.replace(old, new))
                with self.assertRaisesRegex(ValueError, 'version|dependency'):
                    publication.validate_version(self.root, TAG)
                path.write_text(original)
        for tag in ['v0.3.0', 'v01.2.0', 'v0.2.0-rc.1', '0.2.0', 'v0.2']:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                publication.validate_version(self.root, tag)

    def test_tag_and_checkout_must_match_the_triggered_commit(self):
        with patch.object(publication, 'git', return_value=REVISION):
            self.assertEqual(publication.check_tag(self.root, TAG, REVISION), ('0.2.0', REVISION))
        for results in [[REVISION, 'b' * 40], [REVISION, REVISION, 'b' * 40]]:
            with (patch.object(publication, 'git', side_effect=results),
                  self.assertRaisesRegex(ValueError, 'revision')):
                publication.check_tag(self.root, TAG, REVISION)

    def test_remote_tag_accepts_lightweight_and_annotated_tags_but_not_moves(self):
        for output in [f'{REVISION}\trefs/tags/{TAG}\n',
                       f'{"b" * 40}\trefs/tags/{TAG}\n{REVISION}\trefs/tags/{TAG}^{{}}\n']:
            with patch.object(publication.subprocess, 'check_output', return_value=output):
                publication.check_remote_tag('owner/repo', TAG, REVISION)
        moved = f'{"b" * 40}\trefs/tags/{TAG}\n'
        with patch.object(publication.subprocess, 'check_output', return_value=moved):
            with self.assertRaisesRegex(ValueError, 'remote tag'):
                publication.check_remote_tag('owner/repo', TAG, REVISION)

    def test_missing_artifacts_fail_before_network_or_publication(self):
        for path in sorted(self.artifacts.iterdir()):
            original = path.read_bytes()
            path.unlink()
            with (self.subTest(path=path.name),
                  patch.object(publication, 'check_tag', return_value=('0.2.0', REVISION)),
                  patch.object(publication, 'api') as api):
                with self.assertRaisesRegex(ValueError, 'artifacts'):
                    publication.publish(self.root, TAG, REVISION, self.artifacts, 'owner/repo')
                api.assert_not_called()
            path.write_bytes(original)

    def test_artifacts_must_identify_the_release_revision(self):
        binary = self.root / 'binary'
        publication.package.archive(binary, publication.package.TARGETS[0], '0.2.0',
                                    'b' * 40, 1750000000, self.artifacts)
        with self.assertRaisesRegex(ValueError, 'revision'):
            publication.check_artifacts(self.artifacts, '0.2.0', REVISION)

    def test_existing_release_or_draft_is_never_changed(self):
        for draft in [False, True]:
            existing = [{'tag_name': TAG, 'draft': draft}]
            with (patch.object(publication, 'check_tag', return_value=('0.2.0', REVISION)),
                  patch.object(publication, 'check_remote_tag'),
                  patch.object(publication, 'api', return_value=existing) as api):
                with self.assertRaisesRegex(ValueError, 'already exists'):
                    publication.publish(self.root, TAG, REVISION, self.artifacts, 'owner/repo')
                self.assertEqual([call.args[0] for call in api.call_args_list], ['GET'])

    def test_changed_local_artifact_cannot_replace_a_validated_upload(self):
        def api(method, url, data=None):
            if method == 'GET':
                archive = sorted(self.artifacts.glob('*.tar.gz'))[0]
                archive.write_bytes(b'replaced after validation')
                return []
            return {'id': 7, 'draft': True, 'tag_name': TAG}

        with (patch.object(publication, 'check_tag', return_value=('0.2.0', REVISION)),
              patch.object(publication, 'check_remote_tag'),
              patch.object(publication, 'api', side_effect=api) as requests):
            with self.assertRaisesRegex(ValueError, 'validated artifact changed'):
                publication.publish(self.root, TAG, REVISION, self.artifacts, 'owner/repo')
            self.assertEqual([call.args[0] for call in requests.call_args_list], ['GET', 'POST'])

    def test_publication_requires_complete_verified_uploads_and_unchanged_tag(self):
        for failure in [None, 'upload', 'missing', 'digest', 'release', 'tag']:
            with self.subTest(failure=failure):
                draft = {'id': 7, 'draft': True, 'tag_name': TAG,
                         'target_commitish': REVISION, 'assets': []}
                methods = []

                def api(method, url, data=None, content_type='application/json'):
                    methods.append(method)
                    if method == 'GET' and '/releases?' in url:
                        return []
                    if method == 'POST' and url.endswith('/releases'):
                        self.assertTrue(data['draft'])
                        self.assertEqual(data['target_commitish'], REVISION)
                    elif method == 'POST':
                        if failure == 'upload':
                            raise OSError('upload failed')
                        name = parse_qs(urlparse(url).query)['name'][0]
                        draft['assets'].append({
                            'name': name, 'size': len(data), 'state': 'uploaded',
                            'digest': 'sha256:' + hashlib.sha256(data).hexdigest(),
                        })
                    elif method == 'GET':
                        if failure == 'missing':
                            draft['assets'].pop()
                        elif failure == 'digest':
                            draft['assets'][0]['digest'] = 'sha256:' + '0' * 64
                        elif failure == 'release':
                            draft['tag_name'] = 'v0.3.0'
                    elif method == 'PATCH':
                        self.assertEqual(len(draft['assets']), 10)
                        self.assertEqual(data, {'draft': False, 'make_latest': 'legacy'})
                        draft['draft'] = False
                    return copy.deepcopy(draft)
                remote_results = ([None, ValueError('remote tag changed')]
                                  if failure == 'tag' else [None, None])
                with (patch.object(publication, 'check_tag', return_value=('0.2.0', REVISION)),
                      patch.object(publication, 'check_remote_tag', side_effect=remote_results),
                      patch.object(publication, 'api', side_effect=api)):
                    if failure:
                        with self.assertRaises((ValueError, OSError)):
                            publication.publish(self.root, TAG, REVISION, self.artifacts, 'owner/repo')
                        self.assertTrue(draft['draft'])
                        self.assertNotIn('PATCH', methods)
                    else:
                        publication.publish(self.root, TAG, REVISION, self.artifacts, 'owner/repo')
                        self.assertFalse(draft['draft'])
                        self.assertEqual(methods.count('PATCH'), 1)


if __name__ == '__main__':
    unittest.main()
