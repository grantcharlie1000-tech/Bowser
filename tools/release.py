#!/usr/bin/env python3
"""Release helpers for moving the Bowser stable pin. Standard library only.

  python tools/release.py bump 0.0.32      (or: bump next)
      Version-bump PR edits: Cargo.toml, the bowser entry in Cargo.lock,
      PIN_REVISION (+1) in src/updater.rs, and the exact mark-latest allow-list
      entry in stable/allowlist.json. Touches no workflow file, so the
      prepare-release workflow can run it too.

  python tools/release.py pin v0.0.32
      Stable pin PR edit: downloads the published Bowser.exe and
      build-metadata.json, checks them against each other and the source, and
      rewrites stable/release.json (order and four-space indent preserved).

  python tools/release.py check-pin
      Checks stable/release.json against this checkout without building. Used by
      windows-verify for pin-only PRs; mirrors the Rust test
      the_shown_version_follows_the_pinned_release plus the PIN_REVISION rule.
"""
import hashlib
import json
import pathlib
import re
import sys
import tempfile
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
REPO = 'grantcharlie1000-tech/Bowser'
ASSET = 'Bowser.exe'
PIN_FIELDS = ['version', 'revision', 'commit', 'tag', 'repository', 'asset', 'sha256', 'bytes']
REVISION_RE = re.compile(r'^const PIN_REVISION: u32 = (\d+);$', re.M)
ALLOWLIST = 'stable/allowlist.json'
BUMP_FILES = ['Cargo.lock', 'Cargo.toml', 'src/updater.rs', ALLOWLIST]


def fail(message):
    sys.exit(f'release.py: {message}')


def vtuple(text):
    m = re.fullmatch(r'(\d+)\.(\d+)\.(\d+)', str(text))
    if not m:
        fail(f'{text!r} is not MAJOR.MINOR.PATCH')
    return tuple(int(x) for x in m.groups())


def read(rel):
    return (ROOT / rel).read_text(encoding='utf-8')


def write(rel, text):
    (ROOT / rel).write_text(text, encoding='utf-8', newline='\n')


def cargo_version():
    m = re.search(r'(?ms)^\[package\]\n.*?^version = "([^"]+)"$', read('Cargo.toml'))
    if not m:
        fail('no [package] version in Cargo.toml')
    return m.group(1)


def lock_versions():
    return re.findall(r'(?m)^\[\[package\]\]\nname = "bowser"\nversion = "([^"]+)"$', read('Cargo.lock'))


def pin_revision():
    found = REVISION_RE.findall(read('src/updater.rs'))
    if len(found) != 1:
        fail(f'expected exactly one PIN_REVISION in src/updater.rs, found {found}')
    return int(found[0])


def read_allowlist():
    entries = json.loads(read(ALLOWLIST))
    if not isinstance(entries, list) or not all(isinstance(e, dict) and list(e) == ['tag', 'version', 'revision'] for e in entries):
        fail(f'{ALLOWLIST} must be a list of {{tag, version, revision}} objects')
    return entries


def render_allowlist(entries):
    return '[\n' + ',\n'.join('    ' + json.dumps(e) for e in entries) + '\n]\n'


def allowed(tag, version, revision):
    return {'tag': tag, 'version': version, 'revision': revision} in read_allowlist()


def render_pin(pin):
    return json.dumps({k: pin[k] for k in PIN_FIELDS}, indent=4) + '\n'


def check_pin():
    text = read('stable/release.json')
    try:
        pin = json.loads(text)
    except ValueError as err:
        fail(f'stable/release.json is not JSON: {err}')
    problems = []
    if not isinstance(pin, dict) or list(pin) != PIN_FIELDS:
        fail(f'stable/release.json must hold exactly {PIN_FIELDS} in that order')
    if text != render_pin(pin):
        problems.append('stable/release.json must keep four-space indent, field order and a final newline')
    if not isinstance(pin['revision'], int) or isinstance(pin['revision'], bool):
        problems.append('revision must be an integer')
    if not isinstance(pin['bytes'], int) or isinstance(pin['bytes'], bool) or not 512 <= pin['bytes'] <= 536870912:
        problems.append('bytes must be an integer between 512 and 536870912')
    if pin['repository'] != REPO or pin['asset'] != ASSET:
        problems.append(f'repository/asset must be {REPO} / {ASSET}')
    if not re.fullmatch(r'[0-9a-f]{40}', str(pin['commit'])):
        problems.append('commit must be 40 lower-case hex')
    if not re.fullmatch(r'[0-9a-f]{64}', str(pin['sha256'])):
        problems.append('sha256 must be 64 lower-case hex')
    version = vtuple(pin['version'])
    if pin['tag'] != f"v{pin['version']}":
        problems.append(f"tag {pin['tag']!r} must be 'v' + version {pin['version']!r}")
    if vtuple(cargo_version()) < version:
        problems.append(f'Cargo.toml {cargo_version()} is below the pinned {pin["version"]}')
    if lock_versions() != [cargo_version()]:
        problems.append(f'Cargo.lock bowser {lock_versions()} != Cargo.toml {cargo_version()}')
    if pin['revision'] != pin_revision():
        problems.append(f'pin revision {pin["revision"]} != PIN_REVISION {pin_revision()} in src/updater.rs')
    if not allowed(pin['tag'], pin['version'], pin['revision']):
        problems.append(f"{ALLOWLIST} lacks {pin['tag']} revision {pin['revision']}")
    if read(ALLOWLIST) != render_allowlist(read_allowlist()):
        problems.append(f'{ALLOWLIST} must keep one entry per line, four-space indent and a final newline')
    if problems:
        fail('pin check failed:\n  - ' + '\n  - '.join(problems))
    print(f"stable/release.json {pin['tag']} revision {pin['revision']} agrees with the source.")


def bump(new_version):
    old_version = cargo_version()
    if new_version == 'next':
        major, minor, patch = vtuple(old_version)
        new_version = f'{major}.{minor}.{patch + 1}'
    if vtuple(new_version) <= vtuple(old_version):
        fail(f'{new_version} is not above Cargo.toml {old_version}')
    old_revision = pin_revision()
    new_revision = old_revision + 1
    entries = read_allowlist()
    entry = {'tag': f'v{new_version}', 'version': new_version, 'revision': new_revision}
    if entry in entries:
        fail(f'{ALLOWLIST} already has v{new_version}')

    cargo = read('Cargo.toml')
    cargo = re.sub(r'(?ms)(^\[package\]\n.*?^version = ")[^"]+(")', rf'\g<1>{new_version}\g<2>', cargo, count=1)
    write('Cargo.toml', cargo)

    lock, n = re.subn(r'(?m)^(\[\[package\]\]\nname = "bowser"\nversion = ")[^"]+(")$', rf'\g<1>{new_version}\g<2>', read('Cargo.lock'))
    if n != 1:
        fail('could not find exactly one bowser package in Cargo.lock')
    write('Cargo.lock', lock)

    updater = REVISION_RE.sub(f'const PIN_REVISION: u32 = {new_revision};', read('src/updater.rs'), count=1)
    write('src/updater.rs', updater)

    write(ALLOWLIST, render_allowlist(entries + [entry]))
    print(f'Bumped {old_version} -> {new_version}, PIN_REVISION {old_revision} -> {new_revision}, allow-listed v{new_version}.')
    print('Now refresh Cargo.lock offline if needed and run: cargo test --locked --lib updater::')


def fetch(url, dest):
    req = urllib.request.Request(url, headers={'User-Agent': 'bowser-release-pin'})
    with urllib.request.urlopen(req, timeout=300) as resp, open(dest, 'wb') as out:
        while chunk := resp.read(1 << 20):
            out.write(chunk)


def pin(tag):
    version = tag[1:] if tag.startswith('v') else fail(f'tag {tag!r} must start with v')
    vtuple(version)
    revision = pin_revision()
    if cargo_version() != version:
        fail(f'Cargo.toml is {cargo_version()}, not {version}: run this on a branch from the released main')
    base = f'https://github.com/{REPO}/releases/download/{tag}'
    with tempfile.TemporaryDirectory() as tmp:
        exe = pathlib.Path(tmp, ASSET)
        meta_path = pathlib.Path(tmp, 'build-metadata.json')
        fetch(f'{base}/build-metadata.json', meta_path)
        fetch(f'{base}/{ASSET}', exe)
        meta = json.loads(meta_path.read_text(encoding='utf-8-sig'))
        data = exe.read_bytes()
    sha256 = hashlib.sha256(data).hexdigest()
    new = {
        'version': version, 'revision': revision, 'commit': meta.get('commit'), 'tag': tag,
        'repository': REPO, 'asset': ASSET, 'sha256': sha256, 'bytes': len(data),
    }
    for field in PIN_FIELDS:
        if str(meta.get(field)) != str(new[field]):
            fail(f'published build-metadata {field}={meta.get(field)!r} disagrees with {new[field]!r}')
    if meta.get('features') != 'html-chrome-default' or meta.get('smoke_passed') is not True:
        fail('published metadata lacks html-chrome-default smoke provenance')
    write('stable/release.json', render_pin(new))
    print(f'stable/release.json -> {tag} revision {revision}, commit {new["commit"]}, '
          f'sha256 {sha256[:16]}..., {len(data)} bytes (downloaded and hashed).')
    check_pin()


def main(argv):
    if len(argv) == 2 and argv[1] == 'check-pin':
        return check_pin()
    if len(argv) == 3 and argv[1] == 'bump':
        return bump(argv[2])
    if len(argv) == 3 and argv[1] == 'pin':
        return pin(argv[2])
    sys.exit(__doc__)


if __name__ == '__main__':
    main(sys.argv)
