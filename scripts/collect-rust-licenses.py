"""Collect the actual locked dependencies' license files for embedding."""
import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--cargo', default='cargo')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output([
    args.cargo, 'metadata', '--manifest-path', str(root / 'rust/Cargo.toml'),
    '--format-version', '1', '--offline', '--locked'], text=True, encoding='utf-8'))
parts = ['Rust dependency license notices\n\nArtwork: hunmer/aboutme-pets, pets/gpt. '
         'Artwork is separate from the software dependency licenses below.\n']
count = 0
for package in sorted(metadata['packages'], key=lambda p: p['name']):
    if package['name'] == 'codex-pet-rust':
        continue
    folder = Path(package['manifest_path']).parent
    parts.append('\n' + '=' * 72 + '\n' + package['name'] + ' ' + package['version']
                 + '\nLicense: ' + (package.get('license') or 'See license file') + '\n')
    files = sorted(p for p in folder.iterdir() if p.is_file()
                   and p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE')))
    if not files:
        raise RuntimeError('Missing license text: ' + package['name'])
    for file in files:
        parts.append('\n' + file.name + '\n' + file.read_text(encoding='utf-8', errors='replace'))
    count += 1
destination = root / 'outputs/rust-third-party-licenses.txt'
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_text('\n'.join(parts), encoding='utf-8')
print(json.dumps({'dependencies': count, 'noticeBytes': destination.stat().st_size}))
