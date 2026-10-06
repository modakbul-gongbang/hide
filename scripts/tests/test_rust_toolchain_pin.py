"""`rust-toolchain.toml` is the one place the Rust version is written, and every build uses it.

Before the file existed, CI built with whatever stable the runner image carried,
and a new image failed code already on main in every pull request at once. The
pin holds only while nothing restates the version (the copy goes stale on the
next bump) and nothing reaches a toolchain around rustup (that build uses
another version). docs/BUILD.md, "One toolchain version", owns the reasons.
"""
import re
import subprocess
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
PIN = 'rust-toolchain.toml'

# Where a version would be restated: workflows, scripts, documents and crate
# manifests (`rust-version`). Lockfiles name crate versions, not the toolchain.
RESTATE_SCOPE = ['.github', 'scripts', 'docs', '*.md', '*Cargo.toml']

# A toolchain named by its directory, by `+version`, by `rustup run` or by the
# environment builds with that toolchain instead of the pinned one.
BYPASS = re.compile(r'\.rustup/toolchains/|\brustup (?:run|default|override)\b'
                    r'|\b(?:cargo|rustc) \+|\bRUSTUP_TOOLCHAIN\b')


def channel():
    with open(ROOT / PIN, 'rb') as pin:
        return tomllib.load(pin)['toolchain']['channel']


def tracked(*pathspecs):
    return subprocess.check_output(['git', 'ls-files', '--', *pathspecs],
                                   cwd=ROOT, text=True).split()


def restatements(version, files):
    escaped = re.escape(version)
    # A whole token: `1.2.3` is not restated by `11.2.3` or `1.2.30`.
    pattern = re.compile(rf'(?:^|[^0-9.]){escaped}(?:$|[^0-9.]|\.(?:$|[^0-9]))')
    found = []
    for relative in files:
        if relative == PIN:
            continue
        for number, line in enumerate((ROOT / relative).read_text(errors='replace').splitlines(), start=1):
            if pattern.search(line):
                found.append(f'{relative}:{number}: {line.strip()}')
    return found


class RustToolchainPin(unittest.TestCase):
    def test_the_pin_names_an_exact_version_with_the_lint_tools(self):
        with open(ROOT / PIN, 'rb') as pin:
            toolchain = tomllib.load(pin)['toolchain']
        self.assertRegex(toolchain['channel'], r'^\d+\.\d+\.\d+$',
                         'a channel such as `stable` moves under the repository; name a release')
        self.assertTrue({'rustfmt', 'clippy'} <= set(toolchain.get('components', [])),
                        'the rust lane runs rustfmt and Clippy from the pinned toolchain')

    def test_no_workflow_script_or_document_restates_the_version(self):
        found = restatements(channel(), tracked(*RESTATE_SCOPE))
        self.assertEqual(found, [], f'the Rust version is written only in {PIN}; '
                                    'refer to the file instead:\n' + '\n'.join(found))

    def test_a_restated_version_is_found_as_a_whole_token(self):
        version = channel()
        probe = ROOT / 'scripts' / 'tests' / f'.probe-{version}.md'
        try:
            probe.write_text(f'Rust {version}.\nv1{version}\n{version}1\n')
            found = restatements(version, [probe.relative_to(ROOT).as_posix()])
        finally:
            probe.unlink()
        self.assertEqual(len(found), 1, found)

    def test_no_script_or_workflow_reaches_a_toolchain_around_rustup(self):
        found = []
        for relative in tracked('.github/workflows', 'scripts/*.sh', 'scripts/*.mjs', 'desktop/scripts'):
            for number, line in enumerate((ROOT / relative).read_text().splitlines(), start=1):
                if BYPASS.search(line.split('#', 1)[0]):
                    found.append(f'{relative}:{number}: {line.strip()}')
        self.assertEqual(found, [], f'these build with a toolchain other than the one {PIN} names:\n'
                                    + '\n'.join(found))


if __name__ == '__main__':
    unittest.main()
