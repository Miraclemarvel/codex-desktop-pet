import hashlib
import json
from pathlib import Path
import zipfile

root = Path(__file__).resolve().parents[1]
release = root / 'outputs' / 'release' / 'CodexPet'
version = json.loads((root / 'package.json').read_text(encoding='utf-8'))['version']
destination = root / 'outputs' / ('Codex白龙桌宠-Windows-x64-v' + version + '.zip')
entries = sorted(p for p in release.rglob('*') if p.is_file()
                 and p.relative_to(release).parts[:3] != ('resources', 'app', 'outputs'))
def digest(file):
    hasher = hashlib.sha256()
    with file.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(block)
    return hasher.hexdigest()

manifest = [{'path': str(p.relative_to(release)).replace('\\', '/'),
             'bytes': p.stat().st_size,
             'sha256': digest(p)} for p in entries]
(root / 'outputs' / 'release-manifest.json').write_text(json.dumps(manifest, indent=2, ensure_ascii=False), encoding='utf-8')
with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
    for p in entries:
        archive.write(p, Path('CodexPet') / p.relative_to(release))
with zipfile.ZipFile(destination) as archive:
    bad = archive.testzip()
    if bad:
        raise ValueError('Corrupt ZIP entry: ' + bad)
print(json.dumps({'path': str(destination), 'bytes': destination.stat().st_size,
                  'files': len(entries), 'sha256': digest(destination)}, ensure_ascii=False))
