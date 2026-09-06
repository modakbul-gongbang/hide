#!/usr/bin/env python3
"""Check a released runtime at its public and local source boundaries."""
import argparse
import hashlib
import json
import pwd
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / 'macos/Sources/HerdrMacOS/Resources/herdr-bundle.json'


def run(*args, cwd=ROOT):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def public_api(endpoint):
    return json.loads(run('/usr/bin/curl', '--fail', '--silent', '--show-error',
                          f'https://api.github.com/{endpoint}'))


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def source(args):
    checkout = Path(args.checkout).resolve()
    tip = run('git', 'rev-parse', args.branch, cwd=checkout)
    repo = public_api(f'repos/{args.repo}')
    require(repo['fork'], 'release repository is not a fork')
    remote = public_api(f'repos/{args.repo}/branches/{args.branch}')
    require(remote['commit']['sha'] == tip, 'published branch differs from local release tip')
    # Exclude one file from the generic diff, then compare its complete bytes
    # after applying exactly the one approved test expectation to the original.
    run('git', 'diff', '--exit-code', args.branch, '--', '.', ':!src/app/api.rs', cwd=checkout)
    original = (checkout / 'src/app/api.rs').read_text()
    start = original.index('async fn agent_explain_rejects_hook_only_full_lifecycle_authority()')
    end = original.index('\n    #[tokio::test]', start)
    test = original[start:end]
    require(test.count('"agent_not_found"') == 1, 'approved test baseline changed')
    expected = original[:start] + test.replace('"agent_not_found"', '"not_agent_backed"') + original[end:]
    published = run('git', 'show', f'{args.branch}:src/app/api.rs', cwd=checkout)
    require(published == expected.strip(), 'source differs beyond the approved single expectation')
    print(json.dumps({'source': 'pass', 'commit': tip, 'exception': 'one approved test expectation'}))


def asset(args):
    pin = json.loads(MANIFEST.read_text())
    release = public_api(f'repos/{pin["repo"]}/releases/tags/{pin["tag"]}')
    require(release['prerelease'] and not release['draft'], 'release is not a published prerelease')
    require(release['tag_name'] == pin['tag'], 'release tag differs')
    assets = [a for a in release['assets'] if a['name'] == 'herdr-macos-aarch64']
    require(len(assets) == 1 and assets[0]['browser_download_url'] == pin['source_url'], 'release asset differs')
    with tempfile.TemporaryDirectory(prefix='herdr-release-') as directory:
        binary = Path(directory) / 'herdr'
        run('/usr/bin/curl', '--fail', '--location', '--silent', '--show-error', pin['source_url'], '--output', str(binary))
        require(hashlib.sha256(binary.read_bytes()).hexdigest() == pin['sha256'], 'download digest differs')
        binary.chmod(0o755)
        require(run(str(binary), '--version').split()[-1] == pin['version'], 'download version differs')
        schema = json.loads(run(str(binary), 'api', 'schema', '--json'))
        require(schema == json.loads((ROOT / 'contracts/herdr-api.schema.json').read_text()), 'contract differs')
        require(schema == json.loads(run(args.reference_binary, 'api', 'schema', '--json')), 'installed CLI schema differs')
    print(json.dumps({'asset': 'pass', 'repo': pin['repo'], 'tag': pin['tag'], 'sha256': pin['sha256'], 'protocol': schema['protocol']}))


def bump(args):
    pin = json.loads(MANIFEST.read_text())
    unchanged = json.loads(run('zsh', 'scripts/bump-herdr.sh', pin['tag']))
    require(unchanged['outcome'] == 'unchanged', 'current pin does not converge')
    planned = json.loads(run('zsh', 'scripts/bump-herdr.sh', '--repo', 'herdrdev/herdr', 'v0.8.2', '--dry-run'))
    require(planned['outcome'] == 'planned', 'cross-repository dry run is not planned')
    require(planned['source_url'] == 'https://github.com/herdrdev/herdr/releases/download/v0.8.2/herdr-macos-aarch64', 'cross-repository URL differs')
    print(json.dumps({'bump': 'pass', 'current': unchanged, 'upstream': planned}))


def workflow(args):
    text = (ROOT / '.github/workflows/herdr-update.yml').read_text()
    require('gh api repos/herdrdev/herdr/releases/latest' in text, 'upstream stable polling missing')
    require('zsh scripts/bump-herdr.sh --repo herdrdev/herdr "${{ steps.upstream.outputs.latest_tag }}"' in text, 'upstream repository override missing')
    print(json.dumps({'workflow': 'pass'}))


parser = argparse.ArgumentParser(description=__doc__)
subs = parser.add_subparsers(dest='command', required=True)
p = subs.add_parser('source'); p.add_argument('--checkout', default=str(Path(run('git', 'rev-parse', '--path-format=absolute', '--git-common-dir')).parent.parent / 'herdr')); p.add_argument('--repo', required=True); p.add_argument('--branch', default='hide-runtime'); p.set_defaults(fn=source)
p = subs.add_parser('asset'); p.add_argument('--reference-binary', default=str(Path(pwd.getpwuid(os.getuid()).pw_dir) / '.local/bin/herdr')); p.set_defaults(fn=asset)
p = subs.add_parser('bump'); p.set_defaults(fn=bump)
p = subs.add_parser('workflow'); p.set_defaults(fn=workflow)
args = parser.parse_args(); args.fn(args)
