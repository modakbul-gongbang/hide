"""Verification must build this checkout and propagate failures, even when warm.

Use tiny real Cargo/SwiftPM packages at the wrapper boundary. No compiler mocks:
a redirected build directory, a stale external archive, skipped tests, and
swallowed prerequisite failures must change the executable result or exit status.

Every build lands inside the checkout: cargo in `target/`, SwiftPM in
`macos/.build/`. A caller's CARGO_TARGET_DIR must not move the archive SwiftPM
links from its fixed path.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ('toolchain-env.sh', 'verify-cargo.sh', 'verify-swift.sh')


class BuildFixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='hide-build-test-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.env = dict(os.environ, LC_ALL='en_US.UTF-8', LANG='en_US.UTF-8')
        self.env.pop('CARGO_TARGET_DIR', None)

    def checkout(self, parent):
        root = self.base / parent / 'checkout'
        (root / 'scripts').mkdir(parents=True)
        for name in SCRIPTS:
            shutil.copyfile(ROOT / 'scripts' / name, root / 'scripts' / name)
        subprocess.run(['git', 'init', '-q', str(root)], check=True, env=self.env)
        return root


class WrapperArguments(BuildFixture):
    def test_invalid_mode_fails_before_any_build(self):
        root = self.checkout('one')
        for script in ('verify-cargo.sh', 'verify-swift.sh'):
            result = subprocess.run(['bash', 'scripts/' + script, 'invalid'], cwd=root,
                                    env=self.env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertIn('usage:', result.stderr)
        self.assertFalse((root / 'target').exists())


@unittest.skipUnless(sys.platform == 'darwin' and shutil.which('cargo') and shutil.which('swift'),
                     'real archive linking requires macOS, Cargo and SwiftPM')
class RealVerificationBuilds(BuildFixture):
    def run_wrapper(self, root, script, mode=None, *, succeeds=True, expected=41, **extra):
        home, tmp = self.base / 'runner-home', self.base / 'runner-tmp'
        home.mkdir(exist_ok=True)
        tmp.mkdir(exist_ok=True)
        env = dict(self.env, HOME=str(home), TMPDIR=str(tmp) + '/', EXPECTED=str(expected),
                   CARGO_TARGET_DIR=str(self.base / 'must-not-use'), **extra)
        command = ['bash', 'scripts/' + script] + ([mode] if mode else [])
        result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
        if succeeds:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.base / 'must-not-use').exists())
        self.assertFalse((home / '.rustup').exists())
        return result

    def package(self, parent, value):
        root = self.checkout(parent)
        (root / 'herdr-core/src').mkdir(parents=True)
        (root / 'Cargo.toml').write_text('[workspace]\nmembers = ["herdr-core"]\nresolver = "2"\n')
        (root / 'herdr-core/Cargo.toml').write_text(
            '[package]\nname = "herdr-core"\nversion = "0.1.0"\nedition = "2021"\n'
            '[lib]\ncrate-type = ["staticlib", "rlib"]\n')
        self.rust_source(root, value)
        subprocess.run(['bash', '-ec', '. scripts/toolchain-env.sh; cargo generate-lockfile'],
                       cwd=root, env=self.env, check=True, capture_output=True)
        for directory in ('Sources/Bridge', 'Sources/Proof', 'Tests/BridgeTests'):
            (root / 'macos' / directory).mkdir(parents=True)
        (root / 'macos/Package.swift').write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "Fixture", platforms: [.macOS(.v14)], products: [.executable(name: "Proof", targets: ["Proof"])],
    targets: [
        .target(name: "Bridge", linkerSettings: [.unsafeFlags(["-L", "../target/release", "-lherdr_core"])]),
        .executableTarget(name: "Proof", dependencies: ["Bridge"]),
        .testTarget(name: "BridgeTests", dependencies: ["Bridge"])
    ])
''')
        (root / 'macos/Sources/Bridge/Bridge.swift').write_text(
            '@_silgen_name("fixture_value") private func coreValue() -> Int32\n'
            'public func value() -> Int32 { coreValue() }\n')
        (root / 'macos/Sources/Proof/main.swift').write_text('import Bridge\nprint(value())\n')
        (root / 'macos/Tests/BridgeTests/BridgeTests.swift').write_text('''import Testing
import Foundation
import Bridge
@Test func currentCore() {
    #expect(value() == Int32(ProcessInfo.processInfo.environment["EXPECTED"]!)!)
}
''')
        return root

    def rust_source(self, root, value, expected=None):
        (root / 'herdr-core/src/lib.rs').write_text(
            f'#[no_mangle]\npub extern "C" fn fixture_value() -> i32 {{ {value} }}\n'
            f'#[test]\nfn current_value() {{ assert_eq!(fixture_value(), {value if expected is None else expected}); }}\n')

    def output(self, root):
        return subprocess.check_output([str(root / 'macos/.build/debug/Proof')], text=True).strip()

    def test_warm_builds_observe_core_swift_and_test_changes_and_propagate_failures(self):
        root = self.package('one', 41)
        self.run_wrapper(root, 'verify-cargo.sh', 'test')
        self.run_wrapper(root, 'verify-cargo.sh', 'build')
        archive = root / 'target/release/libherdr_core.a'
        before = archive.stat().st_mtime_ns
        self.run_wrapper(root, 'verify-swift.sh', 'build')
        self.run_wrapper(root, 'verify-swift.sh', 'test')
        self.assertEqual(self.output(root), '41')
        self.assertEqual(before, archive.stat().st_mtime_ns)
        self.assertTrue((root / 'target/debug').is_dir(), 'the test build left the checkout')
        self.assertTrue((root / 'macos/.build').is_dir(), 'the Swift build left the checkout')

        self.rust_source(root, 42, expected=41)
        result = self.run_wrapper(root, 'verify-cargo.sh', 'test', succeeds=False)
        self.assertEqual(result.returncode, 101)
        self.rust_source(root, 42)
        self.run_wrapper(root, 'verify-cargo.sh', 'test')
        self.run_wrapper(root, 'verify-swift.sh', 'build', expected=42)
        self.assertEqual(self.output(root), '42', 'Swift executable linked a stale Rust archive')
        self.run_wrapper(root, 'verify-swift.sh', 'test', expected=42)

        bridge = root / 'macos/Sources/Bridge/Bridge.swift'
        bridge.write_text(bridge.read_text().replace('coreValue() }', 'coreValue() * 2 }'))
        self.run_wrapper(root, 'verify-swift.sh', 'test', expected=84)
        self.assertEqual(self.output(root), '84')
        test = root / 'macos/Tests/BridgeTests/BridgeTests.swift'
        test.write_text(test.read_text().replace('#expect(value() ==', '#expect(value() + 1 =='))
        self.run_wrapper(root, 'verify-swift.sh', 'test', succeeds=False, expected=84)
        bridge.write_text('this is not Swift\n')
        self.run_wrapper(root, 'verify-swift.sh', 'build', succeeds=False)
        (root / 'herdr-core/src/lib.rs').write_text('compile_error!("broken core");\n')
        for script, mode in [('verify-cargo.sh', 'build'), ('verify-swift.sh', 'build'),
                             ('verify-swift.sh', 'test')]:
            result = self.run_wrapper(root, script, mode, succeeds=False)
            self.assertIn('broken core', result.stderr)
            self.assertEqual(result.returncode, 101)

        # CI restores the archive by content and links it without a Cargo run;
        # the knob must never turn a missing archive into a rebuild.
        prebuilt = dict(HIDE_CORE_ARCHIVE_PREBUILT='1')
        bridge.write_text('@_silgen_name("fixture_value") private func coreValue() -> Int32\n'
                          'public func value() -> Int32 { coreValue() * 2 }\n')
        test.write_text(test.read_text().replace('#expect(value() + 1 ==', '#expect(value() =='))
        self.run_wrapper(root, 'verify-swift.sh', 'test', expected=84, **prebuilt)
        archive.unlink()
        result = self.run_wrapper(root, 'verify-swift.sh', 'test', succeeds=False, expected=84,
                                  **prebuilt)
        self.assertEqual(result.returncode, 1)
        self.assertIn('libherdr_core.a is missing', result.stderr)


if __name__ == '__main__':
    unittest.main()
