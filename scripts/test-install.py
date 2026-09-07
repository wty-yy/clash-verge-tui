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

    def bundle(self, base, architecture='x86_64'):
        asset = f'clash-verge-tui-v1.4.0-linux-{architecture}.tar.gz'
        archive = self.root / asset
        files = {
            'bin/clash-verge-tui': b'#!/bin/sh\necho "clash-verge-tui 1.4.0"\n',
            'lib/clash-verge-tui/mihomo': b'#!/bin/sh\necho "Mihomo Meta v1.19.29"\n',
            'lib/clash-verge-tui/release.json': b'{"app_version":"1.4.0"}\n',
            'share/licenses/clash-verge-tui/LICENSE': b'fixture TUI license\n',
            'share/licenses/mihomo/LICENSE': b'fixture core license\n',
        }
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
        self.assertTrue((self.home / '.local/lib/clash-verge-tui/MIHOMO-LICENSE').is_file())

    def test_github_default(self):
        repo = 'https://github.com/wty-yy/clash-verge-tui'
        self.responses[repo + '/releases/latest'] = repo + '/releases/tag/v1.4.0'
        self.bundle(repo + '/releases/download/v1.4.0')
        self.assert_installed(self.run_installer())
        self.assertNotIn('gitee.com', self.log.read_text())

    def test_gitee_latest_for_both_architectures(self):
        for architecture in ['x86_64', 'aarch64']:
            with self.subTest(architecture=architecture):
                self.env['TEST_ARCH'] = architecture
                self.responses['https://gitee.com/api/v5/repos/wty-yy/clash-verge-tui/releases/latest'] = json.dumps({'tag_name': 'v1.4.0', 'name': 'Release'}, indent=2)
                self.bundle('https://gitee.com/wty-yy/clash-verge-tui/releases/download/v1.4.0', architecture)
                self.assert_installed(self.run_installer('--source', 'gitee'))
                self.assertNotIn('github.com', self.log.read_text())

    def test_explicit_version_skips_api(self):
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.0'
        self.bundle('https://gitee.com/wty-yy/clash-verge-tui/releases/download/v1.4.0')
        self.assert_installed(self.run_installer('--source', 'gitee'))
        self.assertNotIn('/api/', self.log.read_text())

    def test_repository_override_and_trailing_git(self):
        self.env['CLASH_VERGE_TUI_REPOSITORY'] = 'https://gitee.com/example/mirror.git/'
        self.responses['https://gitee.com/api/v5/repos/example/mirror/releases/latest'] = '{"tag_name":"v1.4.0"}'
        self.bundle('https://gitee.com/example/mirror/releases/download/v1.4.0')
        self.assert_installed(self.run_installer())

    def test_asset_override_and_install_directory(self):
        self.env.update(CLASH_VERGE_TUI_VERSION='v1.4.0', CLASH_VERGE_TUI_ASSET_BASE_URL='https://example.com/assets',
                        CLASH_VERGE_TUI_INSTALL_DIR=str(self.root / 'custom/bin'))
        self.bundle('https://example.com/assets')
        result = self.run_installer('--source', 'gitee')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / 'custom/bin/clash-verge-tui').is_file())
        self.assertNotIn('gitee.com', self.log.read_text())

    def test_missing_release_is_actionable(self):
        result = self.run_installer('--source', 'gitee')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('publish the release bundles', result.stderr)
        self.assertFalse((self.home / '.local').exists())
        self.assertNotIn('github.com', self.log.read_text())

    def test_invalid_release_tag_and_arguments(self):
        self.responses['https://gitee.com/api/v5/repos/wty-yy/clash-verge-tui/releases/latest'] = '{"tag_name":"v1.4.0/../../other"}'
        result = self.run_installer('--source', 'gitee')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('no stable version tag', result.stderr)
        for args in [('--source',), ('--source', 'unknown'), ('--bad',)]:
            self.assertNotEqual(self.run_installer(*args).returncode, 0)
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.0/../../other'
        self.assertIn('invalid release version', self.run_installer().stderr)

    def test_missing_or_corrupt_assets_preserve_installation(self):
        self.env['CLASH_VERGE_TUI_VERSION'] = 'v1.4.0'
        base = 'https://gitee.com/wty-yy/clash-verge-tui/releases/download/v1.4.0'
        archive, checksum = self.bundle(base)
        installed = self.home / '.local/bin/clash-verge-tui'
        installed.parent.mkdir(parents=True)
        installed.write_text('original installation')
        checksum_url = base + '/' + checksum.name
        self.responses.pop(checksum_url)
        self.assertIn('checksum unavailable', self.run_installer('--source', 'gitee').stderr)
        self.responses[checksum_url] = str(checksum)
        archive.write_bytes(b'corrupted archive')
        self.assertNotEqual(self.run_installer('--source', 'gitee').returncode, 0)
        self.assertEqual(installed.read_text(), 'original installation')
        self.responses.pop(base + '/' + archive.name)
        self.assertIn('repository sync alone', self.run_installer('--source', 'gitee').stderr)
        self.assertEqual(installed.read_text(), 'original installation')


if __name__ == '__main__':
    unittest.main()
