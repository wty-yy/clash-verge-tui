#!/usr/bin/env python3
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().with_name('install.sh')
MOCK_CURL = r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
if '--retry-all-errors' in args and os.environ.get('OLD_CURL') == '1':
    print('curl: option --retry-all-errors: is unknown', file=sys.stderr)
    sys.exit(2)
if '--version' in args:
    print('curl test fixture')
    sys.exit(0)
with open(os.environ['REQUEST_LOG'] + '.args', 'a') as log:
    log.write(json.dumps(args) + '\n')
url = args[-1]
with open(os.environ['REQUEST_LOG'], 'a') as log:
    log.write(url + '\n')
responses = json.loads(pathlib.Path(os.environ['RESPONSES']).read_text())
if url not in responses:
    sys.exit(22)
body = responses[url]
if '-w' in args:
    print(body, end='')
elif '-o' in args:
    pathlib.Path(args[args.index('-o') + 1]).write_bytes(pathlib.Path(body).read_bytes())
else:
    print(body)
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='cvt-installer-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bin = self.root / 'tools'
        self.bin.mkdir()
        (self.bin / 'curl').write_text(MOCK_CURL)
        (self.bin / 'curl').chmod(0o755)
        (self.bin / 'uname').write_text('#!/bin/sh\ncase "$1" in -s) echo Linux;; -m) echo "${TEST_ARCH:-x86_64}";; esac\n')
        (self.bin / 'uname').chmod(0o755)
        self.home = self.root / 'home'
        self.home.mkdir()
        self.log = self.root / 'requests'
        self.responses = {}
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('CLASH_VERGE_TUI_')}
        self.env.update(HOME=str(self.home), PATH=str(self.bin) + ':' + os.environ['PATH'],
                        RESPONSES=str(self.root / 'responses.json'), REQUEST_LOG=str(self.log))

    def bundle(self, base, architecture='x86_64', include_geosite=True):
        asset = f'clash-verge-tui-v1.4.5-linux-{architecture}.tar.gz'
        archive = self.root / asset
        files = {
            'bin/clash-verge-tui': b'#!/bin/sh\necho "clash-verge-tui 1.4.5"\n',
            'lib/clash-verge-tui/mihomo': b'#!/bin/sh\necho "Mihomo Meta v1.19.29"\n',
            'lib/clash-verge-tui/GeoSite.dat': b'fixture geosite data\n',
            'lib/clash-verge-tui/release.json': b'{"app_version":"1.4.5"}\n',
            'share/licenses/clash-verge-tui/LICENSE': b'fixture TUI license\n',
            'share/licenses/mihomo/LICENSE': b'fixture core license\n',
            'share/licenses/meta-rules-dat/LICENSE': b'fixture geosite license\n',
        }
        if not include_geosite:
            files.pop('lib/clash-verge-tui/GeoSite.dat')
        with tarfile.open(archive, 'w:gz') as tar:
            for name, content in files.items():
                info = tarfile.TarInfo(name)
                info.size = len(content)
                info.mode = 0o755 if content.startswith(b'#!') else 0o644
                tar.addfile(info, io.BytesIO(content))
        checksum = self.root / (asset + '.sha256')
        checksum.write_text(hashlib.sha256(archive.read_bytes()).hexdigest() + '  ' + asset + '\n')
        self.responses[base + '/' + asset] = str(archive)
        self.responses[base + '/' + asset + '.sha256'] = str(checksum)
        return archive, checksum

    def run_installer(self, *args):
        Path(self.env['RESPONSES']).write_text(json.dumps(self.responses))
        return subprocess.run(['sh', str(INSTALLER), *args], env=self.env,
                              capture_output=True, text=True, timeout=15)

    def assert_installed(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.home / '.local/bin/clash-verge-tui').is_file())
        self.assertTrue((self.home / '.local/lib/clash-verge-tui/core/v1.19.29/mihomo').is_file())
        self.assertTrue((self.home / '.local/lib/clash-verge-tui/GeoSite.dat').is_file())
        self.assertTrue((self.home / '.local/lib/clash-verge-tui/GEOSITE-LICENSE').is_file())
        self.assertTrue((self.home / '.local/lib/clash-verge-tui/MIHOMO-LICENSE').is_file())

    def test_github_default(self):
        repo = 'https://github.com/wty-yy/clash-verge-tui'
        self.responses[repo + '/releases/latest'] = repo + '/releases/tag/v1.4.5'
        self.bundle(repo + '/releases/download/v1.4.5')
        self.assert_installed(self.run_installer())
        self.assertNotIn('gitee.com', self.log.read_text())

    def test_old_curl_omits_unsupported_retry_option(self):
        self.env['OLD_CURL'] = '1'
        repo = 'https://github.com/wty-yy/clash-verge-tui'
        self.responses[repo + '/releases/latest'] = repo + '/releases/tag/v1.4.5'
        self.bundle(repo + '/releases/download/v1.4.5')
        self.assert_installed(self.run_installer())
        self.assertNotIn('--retry-all-errors', Path(str(self.log) + '.args').read_text())

    def test_new_curl_keeps_full_retry_support(self):
        self.test_github_default()
        requests = [json.loads(line) for line in Path(str(self.log) + '.args').read_text().splitlines()]
        self.assertTrue(all('--retry-all-errors' in args for args in requests))

    def test_proxy_without_direct_discovery_for_both_architectures(self):
        for architecture in ['x86_64', 'aarch64']:
            with self.subTest(architecture=architecture):
                self.env['TEST_ARCH'] = architecture
                self.bundle('https://gh-proxy.com/https://github.com/wty-yy/clash-verge-tui/releases/download/v1.4.5', architecture)
                self.assert_installed(self.run_installer('--source', 'proxy'))
                self.assertTrue(all(url.startswith('https://gh-proxy.com/') for url in self.log.read_text().splitlines()))
                self.assertNotIn('/latest', self.log.read_text())

    def test_explicit_version_skips_api(self):
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.5'
        self.bundle('https://gh-proxy.com/https://github.com/wty-yy/clash-verge-tui/releases/download/v1.4.5')
        self.assert_installed(self.run_installer('--source', 'proxy'))
        self.assertNotIn('/api/', self.log.read_text())

    def test_repository_override_and_trailing_git(self):
        self.env['CLASH_VERGE_TUI_REPOSITORY'] = 'https://github.com/example/mirror.git/'
        self.responses['https://github.com/example/mirror/releases/latest'] = 'https://github.com/example/mirror/releases/tag/v1.4.5'
        self.bundle('https://github.com/example/mirror/releases/download/v1.4.5')
        self.assert_installed(self.run_installer())

    def test_asset_override_and_install_directory(self):
        self.env.update(CLASH_VERGE_TUI_VERSION='v1.4.5', CLASH_VERGE_TUI_ASSET_BASE_URL='https://example.com/assets',
                        CLASH_VERGE_TUI_INSTALL_DIR=str(self.root / 'custom/bin'))
        self.bundle('https://example.com/assets')
        result = self.run_installer('--source', 'proxy')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / 'custom/bin/clash-verge-tui').is_file())
        self.assertNotIn('gh-proxy.com', self.log.read_text())

    def test_missing_release_is_actionable(self):
        result = self.run_installer('--source', 'proxy')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('release bundle unavailable', result.stderr)
        self.assertFalse((self.home / '.local').exists())
        self.assertTrue(all(url.startswith('https://gh-proxy.com/') for url in self.log.read_text().splitlines()))

    def test_invalid_release_tag_and_arguments(self):
        self.responses['https://github.com/wty-yy/clash-verge-tui/releases/latest'] = 'https://github.com/wty-yy/clash-verge-tui/releases/tag/invalid'
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid release version', result.stderr)
        for args in [('--source',), ('--source', 'unknown'), ('--source', 'gitee'), ('--github-proxy',), ('--github-proxy', 'http://example.com'), ('--github-proxy', 'https://user:secret@example.com'), ('--bad',)]:
            self.assertNotEqual(self.run_installer(*args).returncode, 0)
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.5/../../other'
        self.assertIn('invalid release version', self.run_installer().stderr)

    def test_missing_or_corrupt_assets_preserve_installation(self):
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.5'
        base = 'https://gh-proxy.com/https://github.com/wty-yy/clash-verge-tui/releases/download/v1.4.5'
        archive, checksum = self.bundle(base)
        installed = self.home / '.local/bin/clash-verge-tui'
        installed.parent.mkdir(parents=True)
        installed.write_text('original installation')
        checksum_url = base + '/' + checksum.name
        self.responses.pop(checksum_url)
        self.assertIn('checksum unavailable', self.run_installer('--source', 'proxy').stderr)
        self.responses[checksum_url] = str(checksum)
        archive.write_bytes(b'corrupted archive')
        self.assertNotEqual(self.run_installer('--source', 'proxy').returncode, 0)
        self.assertEqual(installed.read_text(), 'original installation')
        self.responses.pop(base + '/' + archive.name)
        self.assertIn('release bundle unavailable', self.run_installer('--source', 'proxy').stderr)
        self.assertEqual(installed.read_text(), 'original installation')

    def test_missing_geosite_preserves_installation(self):
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.5'
        self.bundle('https://github.com/wty-yy/clash-verge-tui/releases/download/v1.4.5', include_geosite=False)
        installed = self.home / '.local/bin/clash-verge-tui'
        installed.parent.mkdir(parents=True)
        installed.write_text('original installation')
        self.assertIn('release does not contain GeoSite.dat', self.run_installer().stderr)
        self.assertEqual(installed.read_text(), 'original installation')


if __name__ == '__main__':
    unittest.main()
