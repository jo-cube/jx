#!/usr/bin/env python3
"""Check repository documentation links and run its standalone Rust guide examples."""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent


def check_links():
    guides = [ROOT / name for name in ('README.md', 'ARCHITECTURE.md', 'CONFORMANCE.md',
                                      'PERFORMANCE.md', 'AGENTS.md')]
    guides += list((ROOT / 'docs').glob('*.md')) + list((ROOT / 'crates').glob('*/README.md'))
    checked = 0
    for guide in guides:
        for link in re.findall(r'\]\(([^)]+)\)', guide.read_text(encoding='utf-8')):
            if link.startswith('https://github.com/jo-cube/jx/blob/dev/'):
                path = ROOT / link.split('/blob/dev/', 1)[1].split('#', 1)[0]
            elif ':' in link:
                continue
            else:
                path = guide.parent / unquote(link.split('#', 1)[0]) if not link.startswith('#') else guide
            if not path.exists():
                raise ValueError(f'{guide.relative_to(ROOT)}: broken link {link}')
            if '#' in link and path.is_file() and path.suffix == '.md':
                headings = re.findall(r'^#+\s+(.+)$', path.read_text(encoding='utf-8'), re.MULTILINE)
                anchors = {re.sub(r'[^\w -]', '', heading.lower()).replace(' ', '-') for heading in headings}
                if unquote(link.split('#', 1)[1]) not in anchors:
                    raise ValueError(f'{guide.relative_to(ROOT)}: missing heading {link}')
            checked += 1
    print(f'{checked} documentation links checked', flush=True)


def run_examples():
    sources = [ROOT / name for name in ('README.md', 'docs/embedding.md', 'crates/jx/README.md')]
    target = ROOT / 'target'
    target.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='guide-examples-', dir=target) as temporary:
        package = Path(temporary)
        examples = package / 'examples'
        examples.mkdir()
        (package / 'Cargo.toml').write_text(
            '[package]\nname="jx-guide-examples"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\njx={path="../../crates/jx"}\n', encoding='utf-8')
        shutil.copyfile(ROOT / 'Cargo.lock', package / 'Cargo.lock')
        names = []
        for source in sources:
            for code in re.findall(r'```rust\n(.*?)\n```', source.read_text(encoding='utf-8'), re.DOTALL):
                name = f'guide_{len(names)}'
                (examples / (name + '.rs')).write_text(code + '\n', encoding='utf-8')
                names.append(name)
        if not names:
            raise ValueError('no Rust guide examples found')
        cargo = ['cargo', '--offline']
        subprocess.run(cargo + ['generate-lockfile', '--manifest-path', str(package / 'Cargo.toml')], check=True)
        for name in names:
            subprocess.run(cargo + ['run', '--locked', '--manifest-path', str(package / 'Cargo.toml'),
                                   '--target-dir', str(target), '--example', name], check=True)
        print(f'{len(names)} Rust guide examples passed', flush=True)


if __name__ == '__main__':
    check_links()
    run_examples()
