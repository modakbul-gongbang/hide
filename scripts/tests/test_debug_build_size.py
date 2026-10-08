"""A debug build of the workspace builds each unit once, small.

An agent worktree holds its own `target/`, so its size is paid once per
worktree. It was 8 GB after the commands an agent runs, and three causes made
most of it: a dependency built again for every set of members a cargo command
selected, debug info, and one test binary per `tests/*.rs` file, each linking
every crate again. `cargo hakari` keeps the first away for third-party crates
(`scripts/verify-cargo.sh hakari`); these are the rules it cannot see.
docs/BUILD.md owns the reasons and the measurement.
"""
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent


def manifest(path):
    with open(path, 'rb') as file:
        return tomllib.load(file)


def members():
    return manifest(ROOT / 'Cargo.toml')['workspace']['members']


def dev_dependency_tables(crate):
    """Every `[dev-dependencies]` table of a member, the target-specific ones too."""
    yield crate.get('dev-dependencies', {})
    for target in crate.get('target', {}).values():
        yield target.get('dev-dependencies', {})


class DebugBuildSize(unittest.TestCase):
    def test_each_crate_builds_its_integration_tests_as_one_binary(self):
        found = []
        for member in members():
            tests = ROOT / member / 'tests'
            found += [str(path.relative_to(ROOT)) for path in sorted(tests.glob('*.rs'))]
            found += [str(path.relative_to(ROOT)) for path in sorted(tests.glob('*/main.rs'))
                      if path.parent.name != 'it']
        self.assertEqual(found, [], 'each of these is a test binary that links the crate and every '
                         'dependency again; make it a module of tests/it/main.rs')

    def test_no_dev_dependency_changes_a_members_features(self):
        found = []
        for member in members():
            for table in dev_dependency_tables(manifest(ROOT / member / 'Cargo.toml')):
                for name, spec in table.items():
                    if isinstance(spec, dict) and 'path' in spec and (
                            'features' in spec or spec.get('default-features') is False):
                        found.append(f'{member}: {name}')
        self.assertEqual(found, [], 'a member feature only a test build turns on builds that member '
                         'and every crate above it a second time; reach the test hook at run time')

    def test_the_dev_profile_builds_without_debug_info(self):
        profile = manifest(ROOT / 'Cargo.toml')['profile']['dev']
        self.assertIs(profile.get('debug'), False)
        overrides = [name for name, package in profile.get('package', {}).items() if 'debug' in package]
        self.assertEqual(overrides, [], 'debug info was most of a debug build; '
                         'turn it on for one local build instead (docs/BUILD.md)')


if __name__ == '__main__':
    unittest.main()
