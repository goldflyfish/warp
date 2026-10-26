import argparse
import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package_mywarp as packaging


def write_pe(path, machine=0x8664):
    path.parent.mkdir(parents=True, exist_ok=True)
    contents = bytearray(128)
    contents[:2] = b'MZ'
    struct.pack_into('<I', contents, 60, 64)
    contents[64:68] = b'PE\0\0'
    struct.pack_into('<H', contents, 68, machine)
    path.write_bytes(contents)
    return path


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='mywarp packaging 日本語 ')
        self.root = Path(self.temporary.name)
        self.repo = self.root / 'source repo'
        self.repo.mkdir()
        (self.repo / 'Cargo.toml').write_text('[workspace]\n')
        for name in packaging.DLLS + ('OpenConsole.exe',):
            write_pe(self.repo / 'app/assets/windows/x64' / name)
        for name in ('app/assets/bundled/bootstrap/pwsh.ps1', 'app/channels/oss/icon/no-padding/icon.ico', 'LICENSE-AGPL', 'LICENSE-MIT'):
            target = self.repo / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text('fixture attribution or asset')
        self.resources = self.root / 'prepared resources'
        self.resources.mkdir()
        (self.resources / 'THIRD_PARTY_LICENSES.txt').write_text('fixture third-party licenses')
        (self.resources / 'settings_schema.json').write_text('{"type":"object"}')
        (self.resources / 'bundled').mkdir()
        (self.resources / 'bundled/fixture.txt').write_text('fixture bundled resource')
        self.binary = write_pe(self.root / 'binaries/warp-oss.exe')
        self.opencode = write_pe(self.root / 'opencode/bin/opencode.exe')
        (self.opencode.parent.parent / 'LICENSE').write_text('fixture OpenCode license')
        self.output = self.root / 'output files'
        self.arguments = ['--repo', str(self.repo), '--skip-build', '--binary', str(self.binary), '--resources-dir', str(self.resources), '--opencode', str(self.opencode), '--output-dir', str(self.output), '--arch', 'x64']

    def tearDown(self):
        self.temporary.cleanup()

    def invoke(self, mode='portable', extra=()):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return packaging.main(mode, self.arguments + list(extra))

    def test_portable_zip_contains_only_fresh_payload_and_portable_configuration(self):
        (self.repo / 'local-private.key').write_text('DO_NOT_DISTRIBUTE_THIS_FIXTURE_SECRET')
        self.assertEqual(self.invoke(), 0)
        folder = self.output / 'MyWarp-0.1.0-x64-portable'
        self.assertTrue((folder / 'mywarp-portable').is_file())
        self.assertFalse((folder / 'mywarp-installed').exists())
        self.assertTrue((folder / 'tools/opencode/LICENSE.txt').is_file())
        config = folder / 'profile/providers/config/opencode/opencode.jsonc'
        self.assertEqual(json.loads(config.read_text()), packaging.DEFAULT_CONFIG)
        with zipfile.ZipFile(self.output / (folder.name + '.zip')) as archive:
            self.assertIn(folder.name + '/MyWarp.exe', archive.namelist())
            self.assertFalse(any(name.endswith('openrouter.key') for name in archive.namelist()))
            self.assertFalse(any(b'DO_NOT_DISTRIBUTE_THIS_FIXTURE_SECRET' in archive.read(name) for name in archive.namelist()))

    def test_existing_portable_profile_is_preserved_when_rebuilding(self):
        self.assertEqual(self.invoke(), 0)
        key = self.output / 'MyWarp-0.1.0-x64-portable/profile/user.key'
        key.write_text('existing-private-fixture')
        self.assertEqual(self.invoke(), 1)
        self.assertEqual(key.read_text(), 'existing-private-fixture')

    def test_wrong_runtime_architecture_fails_before_creating_output(self):
        write_pe(self.opencode, machine=0xAA64)
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(self.output.exists())

    def test_git_lfs_pointer_in_place_of_a_dll_is_rejected(self):
        (self.repo / 'app/assets/windows/x64/conpty.dll').write_text('version https://git-lfs.github.com/spec/v1\noid fixture')
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(list(self.output.glob('*.zip')))
        self.assertFalse(list(self.output.glob('.mywarp-package-*')))

    def test_missing_attributions_prevent_distribution(self):
        (self.resources / 'THIRD_PARTY_LICENSES.txt').unlink()
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(list(self.output.glob('*.zip')))

    def test_malformed_settings_schema_prevents_distribution(self):
        (self.resources / 'settings_schema.json').write_text('not-json')
        self.assertEqual(self.invoke(), 1)
        self.assertFalse(list(self.output.glob('*.zip')))

    def test_version_cannot_inject_paths_or_inno_directives(self):
        self.assertEqual(self.invoke(extra=['--version', '../escape']), 1)
        self.assertFalse(self.output.exists())

    def test_installer_payload_uses_its_own_marker_and_no_portable_profile(self):
        payload = self.root / 'installed payload'
        args = argparse.Namespace(repo=self.repo, skip_build=True, resources_dir=self.resources, opencode_license=None)
        packaging.stage_payload(args, 'installer', 'x64', self.binary, self.opencode, payload)
        self.assertTrue((payload / 'mywarp-installed').is_file())
        self.assertFalse((payload / 'mywarp-portable').exists())
        self.assertFalse((payload / 'profile').exists())
        wrapper = (payload / 'tools/bin/opencode.cmd').read_text()
        self.assertIn('%LOCALAPPDATA%\\MyWarp', wrapper)
        self.assertIn('set "OPENCODE_CONFIG_CONTENT="', wrapper)

    def test_failed_installer_compiler_leaves_no_partial_distribution(self):
        iscc = self.root / 'ISCC.exe'
        iscc.write_bytes(b'fixture')
        def fail_compiler(*args, **kwargs):
            raise packaging.PackagingError('fixture compiler failure')
        with patch.object(packaging, 'run', side_effect=fail_compiler):
            self.assertEqual(self.invoke('installer', ['--iscc', str(iscc)]), 1)
        self.assertFalse(list(self.output.glob('*Setup.exe')))
        self.assertFalse(list(self.output.glob('.mywarp-package-*')))

    def test_build_reuses_configured_cargo_target_directory(self):
        args = argparse.Namespace(skip_build=False, binary=None, resources_dir=None, profile='dev', version='0.1.0', repo=self.repo, arch=None)
        target = self.root / 'custom build target'
        expected = write_pe(target / 'debug/warp-oss.exe')
        with patch.object(packaging.shutil, 'which', return_value='fixture-tool'), patch.object(packaging, 'run'), patch.object(packaging.subprocess, 'check_output', return_value=json.dumps({'target_directory': str(target)})), patch.dict(packaging.os.environ, {'CARGO_BUILD_TARGET': ''}):
            self.assertEqual(packaging.build_binary(args, 'x64'), expected)


if __name__ == '__main__':
    unittest.main()
