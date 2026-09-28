#!/usr/bin/env python3
"""Exercise the POSIX installer with local release assets and no network."""
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='dovetail installer ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.destination = self.root / 'install dir'
        self.destination.mkdir()
        self.executable = self.destination / 'dovetail'
        self.executable.write_text('existing installation')
        self.assets = self.root / 'assets'
        self.assets.mkdir()
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                        ASSETS=str(self.assets), SYSTEM='Linux', ARCH='x86_64')
        self.stub('uname', 'case "$1" in -s) echo "$SYSTEM" ;; -m) echo "$ARCH" ;; esac')
        self.stub('curl', '''
output=
for arg in "$@"; do
    if [ "${previous:-}" = '-o' ]; then output=$arg; fi
    case "$arg" in https://*) url=$arg ;; esac
    previous=$arg
done
case "$url" in
    */latest) echo 'https://github.com/somdoron/dovetail/releases/tag/v0.1.4' ;;
    */download/v0.1.4/*) cp "$ASSETS/${url##*/}" "$output" ;;
    *) exit 22 ;;
esac
''')

    def stub(self, name, body):
        path = self.bin / name
        path.write_text('#!/bin/sh\nset -eu\n' + body + '\n')
        path.chmod(0o755)

    def asset(self, target, body=b'#!/bin/sh\necho dovetail 0.1.4\n'):
        name = f'dovetail-{target}'
        (self.assets / name).write_bytes(body)
        checksum = self.assets / (name + '.sha256')
        checksum.write_text(f'{hashlib.sha256(body).hexdigest()}  {name}\n')
        return checksum

    def run_installer(self, *args):
        return subprocess.run(['sh', str(ROOT / 'website/installers/install.sh'), '--install-dir',
                               str(self.destination), *args], env=self.env,
                              capture_output=True, text=True)

    def test_supported_platforms_and_upgrade(self):
        for system, arch, target in [
            ('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
            ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu'),
            ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
            ('Darwin', 'arm64', 'aarch64-apple-darwin'),
        ]:
            with self.subTest(system=system, arch=arch):
                self.env.update(SYSTEM=system, ARCH=arch)
                self.asset(target)
                result = self.run_installer('--version', 'v0.1.4')
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(os.access(self.executable, os.X_OK))
                self.assertEqual(list(self.destination.glob('.dovetail.*')), [])

    def test_checksum_failure_preserves_installation(self):
        checksum = self.asset('x86_64-unknown-linux-gnu')
        checksum.write_text('0' * 64 + '  dovetail-x86_64-unknown-linux-gnu\n')
        result = self.run_installer('--version', 'v0.1.4')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Checksum mismatch', result.stderr)
        self.assertEqual(self.executable.read_text(), 'existing installation')

    def test_ambiguous_checksum_preserves_installation(self):
        checksum = self.asset('x86_64-unknown-linux-gnu')
        checksum.write_text(checksum.read_text() * 2)
        self.assertNotEqual(self.run_installer('--version', 'v0.1.4').returncode, 0)
        self.assertEqual(self.executable.read_text(), 'existing installation')

    def test_unrunnable_binary_preserves_installation(self):
        self.asset('x86_64-unknown-linux-gnu', b'#!/bin/sh\nexit 1\n')
        self.assertNotEqual(self.run_installer('--version', 'v0.1.4').returncode, 0)
        self.assertEqual(self.executable.read_text(), 'existing installation')

    def test_latest_resolution_and_fresh_install(self):
        self.executable.unlink()
        self.asset('x86_64-unknown-linux-gnu')
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.executable.exists())

    def test_invalid_input_and_unsupported_platform(self):
        for args in [('--version', '../bad'), ('--unknown',), ('--version',)]:
            self.assertNotEqual(self.run_installer(*args).returncode, 0)
        self.env['ARCH'] = 'riscv64'
        self.assertNotEqual(self.run_installer('--version', 'v0.1.4').returncode, 0)
        self.assertEqual(self.executable.read_text(), 'existing installation')


if __name__ == '__main__':
    unittest.main()
