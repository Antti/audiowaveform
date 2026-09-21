#!/usr/bin/env python3
"""Record locked Cargo component notices and exact source-package locations."""
import hashlib
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parents[3]
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--offline', '--locked', '--format-version', '1',
    '--manifest-path', str(root / 'bindings/ruby/Cargo.toml')], text=True))
out = root / 'third-party'
out.mkdir(exist_ok=True)
packages = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if package['source'] is None:
        continue
    source = pathlib.Path(package['manifest_path']).parent
    files = sorted(p for p in source.iterdir() if p.is_file() and
                   p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'UNLICENSE')))
    if package.get('license_file'):
        files = sorted(set(files + [source / package['license_file']]))
    if not files:
        raise RuntimeError(f"No license text located for {package['name']}")
    retained = []
    for path in files:
        data = path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        target = out / (digest + '.txt')
        target.write_bytes(data)
        retained.append({'original': path.name, 'file': 'third-party/' + target.name, 'sha256': digest})
    packages.append({'name': package['name'], 'version': package['version'],
                     'license': package['license'], 'repository': package['repository'],
                     'source': f"https://crates.io/api/v1/crates/{package['name']}/{package['version']}/download",
                     'notices': retained})
(out / 'manifest.json').write_text(json.dumps(packages, indent=2) + '\n')
lines = ['# Third-party components', '',
         'This inventory covers the locked Ruby extension Cargo graph, including',
         'build-time and target-conditional crates. It is broader than the components',
         'linked into any one binary. Original license/notice texts are retained under',
         '`third-party/`; identical texts share a content-addressed file.', '',
         'Symphonia is unmodified MPL-2.0 code. Its covered source is available in the',
         "exact versioned source packages below. These notices supplement this project's",
         'dual-license choice in [LICENSE.md](LICENSE.md). Ruby itself is supplied by the',
         'user, not bundled into the gem. Build-tool Ruby gems are also not bundled.', '',
         'Regenerate with `python3 bindings/ruby/script/update_notices.py` after',
         'changing the locked dependency graph. Before publishing native gems,',
         'verify their actual linked components and source availability.', '',
         '| Component | Declared license | Original texts | Exact source |',
         '| --- | --- | --- | --- |']
for p in packages:
    texts = ', '.join(f"[{n['original']}]({n['file']})" for n in p['notices'])
    lines.append(f"| {p['name']} {p['version']} | {p['license']} | {texts} | [source]({p['source']}) |")
(root / 'THIRD-PARTY-NOTICES.md').write_text('\n'.join(lines) + '\n')
print(f"Recorded notices for {len(packages)} locked external crates")
