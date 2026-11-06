import contextlib
import hashlib
import io
import json
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]


class WorkerPreparation(unittest.TestCase):
    def prepare(self, destination, jkq, mode, engine=None):
        actual_run = subprocess.run
        commands = []

        def run(command, **kwargs):
            commands.append(command)
            if command[0] == 'cargo':
                manifest = Path(command[command.index('--manifest-path') + 1])
                name = 'jx-absolute-default' if mode == '--default-only' else 'simd-reference'
                stale = manifest.parent / 'target/release' / name
                stale.parent.mkdir(parents=True, exist_ok=True)
                stale.write_bytes(b'stale binary')
                binary = destination / 'external-target/release' / name
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(b'benchmark binary')
                provider = engine or ROOT / 'crates/jx'
                def artifact(target, source, executable):
                    return dict(reason='compiler-artifact', package_id=str(source),
                                manifest_path=str(source), target=dict(name=target, kind=['bin' if executable else 'lib']),
                                profile={}, features=[], filenames=[str(binary)] if executable else [],
                                executable=str(executable) if executable else None)
                messages = [artifact('jx', provider / 'Cargo.toml', None),
                            artifact(name, manifest, binary)]
                if engine is not None:
                    messages[0]['features'] = ['default', 'probe']
                return subprocess.CompletedProcess(command, 0,
                    stdout='\n'.join(json.dumps(message) for message in messages))
            if command[0] == 'python3':
                return actual_run(command, capture_output=True, text=True, **kwargs)
            return actual_run(command, **kwargs)

        with patch.object(sys, 'argv', ['prepare.py', str(destination), '--jkq', str(jkq), mode]), \
                patch('subprocess.run', side_effect=run), contextlib.redirect_stdout(io.StringIO()):
            try:
                runpy.run_path(str(ROOT / 'benchmarks/absolute/prepare.py'), run_name='__main__')
            except SystemExit as error:
                self.assertEqual(error.code, 0)
        return commands

    def test_default_only_copies_unmodified_worker_and_records_pending_sources(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            jkq = root / 'jkq'
            jkq.mkdir()
            subprocess.run(['git', 'init', '--quiet', str(jkq)], check=True)
            sources = {
                'mod.rs': b'// current transform plan\n',
                'jsonata.rs': b'// complete-result worker; no historical serializer\n',
                'jsonata/tests.rs': b'use crate::transform::build_plan;\n',
            }
            for name, content in sources.items():
                path = jkq / 'src/transform' / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(content)
            subprocess.run(['git', '-C', str(jkq), 'add', '.'], check=True)
            subprocess.run(['git', '-C', str(jkq), '-c', 'user.name=Fixture',
                            '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false',
                            'commit', '--quiet',
                            '-m', 'fixture'], check=True)
            revision = subprocess.check_output(['git', '-C', str(jkq), 'rev-parse', 'HEAD'], text=True).strip()
            sources['jsonata.rs'] += b'// pending integration\n'
            (jkq / 'src/transform/jsonata.rs').write_bytes(sources['jsonata.rs'])
            destination = root / 'prepared'
            engine = root / 'override-jx'
            (engine / 'src').mkdir(parents=True)
            (engine / 'Cargo.toml').write_text('[package]\nname="jx"\nversion="0.2.0"\n'
                                                '[features]\ndefault=["probe"]\nprobe=[]\n')
            (engine / 'src/lib.rs').write_text('// external dependency provider\n')
            commands = self.prepare(destination, jkq, '--default-only', engine)
            self.assertFalse((destination / 'worker/foundation').exists())
            self.assertFalse((destination / 'worker/worker').exists())
            for name, content in sources.items():
                self.assertEqual((destination / 'worker/reference' / name).read_bytes(), content)
                self.assertEqual((jkq / 'src/transform' / name).read_bytes(), content)
            build = json.loads((destination / 'default-build.json').read_text())
            self.assertEqual(build['mode'], 'default-only')
            self.assertEqual(build['worker']['revision'], revision)
            self.assertTrue(build['worker']['dirty'])
            self.assertEqual(build['worker']['source_sha256'], {
                name: hashlib.sha256(content).hexdigest() for name, content in sources.items()})
            self.assertEqual(build['binary_sha256'], hashlib.sha256(b'benchmark binary').hexdigest())
            self.assertIn('external-target', build['binary'])
            self.assertEqual(build['features'], ['default', 'probe'])
            self.assertEqual(build['cargo']['engine']['manifest_path'], str((engine / 'Cargo.toml').resolve()))
            self.assertIsNone(build['cargo']['engine']['revision'])
            self.assertIsNone(build['cargo']['engine']['dirty'])
            for relative, expected in build['generated_sha256'].items():
                self.assertEqual(expected, hashlib.sha256(
                    (destination / 'default-harness' / relative).read_bytes()).hexdigest())
            generated_test = 'src/reference/jsonata/tests.rs'
            self.assertIn(generated_test, build['generated_sha256'])
            self.assertNotEqual(build['generated_sha256'][generated_test],
                                build['worker']['source_sha256']['jsonata/tests.rs'])
            cargo = [command for command in commands if command[0] == 'cargo']
            self.assertEqual(len(cargo), 1)
            self.assertIn('--locked', cargo[0])
            self.assertIn('--offline', cargo[0])

    def test_simd_only_needs_no_worker_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / 'prepared'
            commands = self.prepare(destination, Path(temporary) / 'absent-jkq', '--simd-only')
            self.assertFalse((destination / 'worker').exists())
            self.assertFalse(any(command[0] == 'python3' for command in commands))
            build = json.loads((destination / 'simd-build.json').read_text())
            self.assertEqual(build['mode'], 'simd-only')
            self.assertNotIn('worker', build)
            cargo = [command for command in commands if command[0] == 'cargo']
            self.assertEqual(cargo[0][cargo[0].index('--bin') + 1], 'simd-reference')
