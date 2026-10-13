#!/usr/bin/env python3
"""Exercise the real installer with local release fixtures; no network or root needed."""
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / 'install.sh'
TARGETS = {
    ('Linux', 'x86_64'): 'x86_64-unknown-linux-musl',
    ('Linux', 'aarch64'): 'aarch64-unknown-linux-musl',
    ('Darwin', 'x86_64'): 'x86_64-apple-darwin',
    ('Darwin', 'arm64'): 'aarch64-apple-darwin',
}


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'mock-bin'
        self.bin.mkdir()
        self.release = self.root / 'release'
        self.release.mkdir()
        self.destination = self.root / 'install with spaces'
        self.destination.mkdir()
        self.existing = self.destination / 'terminapp'
        self.existing.write_text('old installation')
        sums = []
        for target in TARGETS.values():
            name = f'terminapp-{target}.tar.gz'
            payload = b'#!/bin/sh\necho terminapp 0.1.0\n'
            with tarfile.open(self.release / name, 'w:gz') as archive:
                entry = tarfile.TarInfo('terminapp')
                entry.size, entry.mode = len(payload), 0o755
                archive.addfile(entry, io.BytesIO(payload))
            sums.append(hashlib.sha256((self.release / name).read_bytes()).hexdigest() + '  ' + name)
        (self.release / 'SHA256SUMS').write_text('\n'.join(sums) + '\n')
        self.mock('uname', '#!/bin/sh\ncase "$1" in -s) echo "$MOCK_OS";; -m) echo "$MOCK_ARCH";; esac\n')
        self.mock('curl', '''#!/usr/bin/env python3
import os, pathlib, shutil, sys
args = sys.argv[1:]
assert args[args.index('--proto') + 1] == '=https'
assert '--fail' in args
url = next(a for a in args if a.startswith('https://'))
assert url.startswith('https://github.com/alveflo/terminal-news/releases/')
pathlib.Path(os.environ['MOCK_LOG']).open('a').write(url + '\\n')
if os.environ.get('MOCK_FAIL'):
    sys.exit(22)
shutil.copyfile(pathlib.Path(os.environ['MOCK_RELEASE']) / url.rsplit('/', 1)[1], args[args.index('-o') + 1])
''')

    def mock(self, name, contents):
        path = self.bin / name
        path.write_text(contents)
        path.chmod(0o755)

    def run_installer(self, system='Linux', arch='x86_64', **extra):
        env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                   MOCK_OS=system, MOCK_ARCH=arch, MOCK_RELEASE=str(self.release),
                   MOCK_LOG=str(self.root / 'requests'), TERMINAPP_INSTALL_DIR=str(self.destination))
        env.pop('TERMINAPP_VERSION', None)
        env.update(extra)
        return subprocess.run(['sh', str(INSTALLER)], env=env, capture_output=True, text=True)

    def test_platforms_and_atomic_upgrade(self):
        for system, arch in TARGETS:
            with self.subTest(system=system, arch=arch):
                result = self.run_installer(system, arch)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(os.access(self.existing, os.X_OK))
                self.assertEqual(subprocess.check_output([str(self.existing)], text=True).strip(), 'terminapp 0.1.0')
                self.assertFalse(list(self.destination.glob('.terminapp.*')))

    def test_pinned_version(self):
        result = self.run_installer(TERMINAPP_VERSION='v0.1.0')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('/download/v0.1.0/', (self.root / 'requests').read_text())

    def test_checksum_mismatch_preserves_existing_binary(self):
        checksum = self.release / 'SHA256SUMS'
        checksum.write_text('0' * 64 + '  terminapp-x86_64-unknown-linux-musl.tar.gz\n')
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Checksum verification failed', result.stderr)
        self.assertEqual(self.existing.read_text(), 'old installation')

    def test_missing_checksum(self):
        (self.release / 'SHA256SUMS').write_text('')
        self.assertNotEqual(self.run_installer().returncode, 0)
        self.assertEqual(self.existing.read_text(), 'old installation')

    def test_download_failure(self):
        self.assertNotEqual(self.run_installer(MOCK_FAIL='1').returncode, 0)
        self.assertEqual(self.existing.read_text(), 'old installation')

    def test_unsupported_platform_and_invalid_version(self):
        self.assertNotEqual(self.run_installer('FreeBSD', 'x86_64').returncode, 0)
        self.assertNotEqual(self.run_installer('Linux', 'i686').returncode, 0)
        self.assertNotEqual(self.run_installer(TERMINAPP_VERSION='../../other').returncode, 0)
        self.assertFalse((self.root / 'requests').exists())


if __name__ == '__main__':
    unittest.main()
