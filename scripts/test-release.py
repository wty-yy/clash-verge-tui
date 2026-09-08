#!/usr/bin/env python3
"""Exercise an actual release installer in an isolated temporary home."""
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile

archive = Path(sys.argv[1]).resolve()
match = re.fullmatch(r'clash-verge-tui-(v\d+\.\d+\.\d+)-linux-(x86_64|aarch64)\.tar\.gz', archive.name)
assert match, archive.name
version = match.group(1)
installer = Path(__file__).resolve().with_name('install.sh')
with tempfile.TemporaryDirectory(prefix='cvt-release-test-') as directory:
    root = Path(directory)
    home = root / 'home'
    home.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith(('CLASH_VERGE_', 'XDG_'))}
    env.update(HOME=str(home), XDG_DATA_HOME=str(home / '.local/share'),
               XDG_CONFIG_HOME=str(home / '.config'), XDG_CACHE_HOME=str(home / '.cache'),
               CLASH_VERGE_TUI_VERSION=version,
               CLASH_VERGE_TUI_ASSET_BASE_URL=archive.parent.as_uri())
    subprocess.run(['sh', str(installer)], env=env, check=True, timeout=90)
    app = home / '.local/bin/clash-verge-tui'
    assert subprocess.check_output([str(app), '--version'], env=env, text=True).strip() == 'clash-verge-tui ' + version[1:]
    core = home / '.local/lib/clash-verge-tui/core/v1.19.29/mihomo'
    geosite = home / '.local/lib/clash-verge-tui/GeoSite.dat'
    assert (home / '.local/lib/clash-verge-tui/MUSL-LICENSE').is_file()
    assert geosite.is_file()
    assert (home / '.local/lib/clash-verge-tui/GEOSITE-LICENSE').is_file()
    expected_hash = hashlib.sha256(core.read_bytes()).digest()
    geosite_hash = hashlib.sha256(geosite.read_bytes()).hexdigest()
    metadata = json.loads((geosite.parent / 'release.json').read_text())
    assert geosite_hash == metadata['geosite_sha256']
    workspace = root / 'workspace'
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    command = [str(app), '--check', '--data-dir', str(workspace), '--mixed-port', str(port), '--language', 'en']
    for repair in [False, True]:
        if repair:
            (workspace / 'core/mihomo').write_bytes(b'corrupted test core\n')
            (workspace / 'core/GeoSite.dat').unlink()
        else:
            workspace.mkdir()
            # Exercise initial validation with real GeoSite rules and a dead download URL.
            manifest = {'profiles': [], 'active': 0, 'state': {'overrides': {
                'rules': ['GEOSITE,cn,DIRECT', 'MATCH,DIRECT'],
                'geox-url': {'geosite': 'http://127.0.0.1:1/blocked.dat'},
                'geo-auto-update': False}}}
            (workspace / 'workspace-state.json').write_text(json.dumps(manifest))
            (workspace / 'workspace-state.json').chmod(0o600)
        result = subprocess.run(command, env=env, check=True, capture_output=True, text=True, timeout=90)
        status = json.loads(result.stdout)
        assert status['app_version'] == version[1:], status
        assert status['expected_core'] == 'v1.19.29', status
        assert status['managed'] is True, status
        assert hashlib.sha256((workspace / 'core/mihomo').read_bytes()).digest() == expected_hash
        assert hashlib.sha256((workspace / 'core/GeoSite.dat').read_bytes()).hexdigest() == geosite_hash
        assert (workspace / 'core/GeoSite.dat').stat().st_mode & 0o777 == 0o600
        validation_log = (workspace / 'core/validation.log').read_text()
        assert 'start download' not in validation_log.lower(), validation_log
        assert (workspace / 'workspace-state.json').stat().st_mode & 0o777 == 0o600
        for private in workspace.rglob('*'):
            if private.is_file() and (private.suffix in ['.yaml', '.json'] or private.name.endswith('.secret')):
                assert private.stat().st_mode & 0o777 == 0o600, private.name
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', port))
        for proc in Path('/proc').glob('[0-9]*/cmdline'):
            try:
                assert str(workspace / 'core/mihomo').encode() not in proc.read_bytes(), 'managed core survived --check'
            except (FileNotFoundError, PermissionError, ProcessLookupError):
                pass
    for width, height in [(76, 24), (120, 40)]:
        subprocess.run([str(app), '--snapshot', 'home', '--width', str(width), '--height', str(height),
                        '--output', str(root / f'home-{width}.txt'), '--language', 'en'],
                       env=env, check=True, timeout=20)
    print('PASS: real installer, offline GeoSite validation, core version, private permissions, process cleanup, core/data repair, and snapshots')
