"""Build the embedded asset archive without changing any source image bytes."""
import hashlib
import json
from pathlib import Path
import struct
import zlib

root = Path(__file__).resolve().parents[1]
assets = root / 'assets'
provenance = json.loads((assets / 'SOURCE.json').read_text(encoding='utf-8'))
checksums = {item['path']: item['sha256'] for item in provenance['files']}
animations = json.loads((assets / 'upstream-actions.json').read_text(encoding='utf-8'))['animations']
payload = bytearray()
original_bytes = 0
entries = []
for animation in animations:
    relative = animation['sources']['webp']
    original = (assets / relative).read_bytes()
    assert hashlib.sha256(original).hexdigest() == checksums[relative], relative
    compressed = zlib.compress(original, 9)
    assert zlib.decompress(compressed) == original, relative
    entries.append({**animation, 'offset': len(payload), 'length': len(compressed),
                    'originalLength': len(original), 'sha256': checksums[relative]})
    payload.extend(compressed)
    original_bytes += len(original)
metadata = json.dumps({'animations': entries, 'source': provenance,
                      'rustLicenses': (root / 'outputs/rust-third-party-licenses.txt').read_text(encoding='utf-8'),
                      'upstreamReadme': (assets / 'UPSTREAM-README.md').read_text(encoding='utf-8'),
                      'notices': (root / 'THIRD_PARTY_NOTICES.md').read_text(encoding='utf-8'),
                      'dockArtwork': json.loads((assets / 'EDGE_SOURCE.json').read_text(encoding='utf-8'))},
                     ensure_ascii=False, separators=(',', ':')).encode('utf-8')
destination = root / 'outputs' / 'rust-assets.bin'
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_bytes(b'CDXRUST1' + struct.pack('<I', len(metadata)) + metadata + payload)
result = {'animations': len(entries), 'originalWebPBytes': original_bytes,
          'compressedWebPBytes': len(payload), 'archiveBytes': destination.stat().st_size,
          'sourceBytesPreserved': True, 'sha256': hashlib.sha256(destination.read_bytes()).hexdigest()}
(root / 'outputs' / 'rust-assets-verification.json').write_text(json.dumps(result, indent=2), encoding='utf-8')
print(json.dumps(result))
