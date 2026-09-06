#!/usr/bin/env python3
"""Check a released runtime at its public and local source boundaries."""
import argparse
import hashlib
import json
import pwd
import re
import shutil
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
    baseline = '4ef0414d32426c98dada52272708d1de2efa4a94'
    run('git', 'diff', '--exit-code', baseline, '--', '.', cwd=checkout)
    commits = run('git', 'rev-list', '--reverse', f'{baseline}..{args.branch}', cwd=checkout).splitlines()
    require(len(commits) == 2, 'expected the approved expectation and fixture commits')
    tests = ['agent_explain_rejects_hook_only_full_lifecycle_authority',
             'live_handoff_keeps_unmanaged_agent_name_bound_to_saved_session']
    files = ['src/app/api.rs', 'tests/live_handoff.rs']
    for commit, test, path in zip(commits, tests, files):
        require(test in run('git', 'show', '-s', '--format=%B', commit, cwd=checkout), 'fix commit must name its test')
        require(run('git', 'diff-tree', '--no-commit-id', '--name-only', '-r', commit, cwd=checkout) == path, 'fix changed unapproved files')
    # Production source stays byte-identical except for the approved test-module assertion.
    run('git', 'diff', '--exit-code', baseline, args.branch, '--', '.', ':!tests/live_handoff.rs', ':!src/app/api.rs', cwd=checkout)
    original = subprocess.check_output(['git', 'show', f'{baseline}:src/app/api.rs'], cwd=checkout).decode()
    start = original.index('async fn agent_explain_rejects_hook_only_full_lifecycle_authority()')
    end = original.index('\n    #[tokio::test]', start)
    test = original[start:end]
    expected = original[:start] + test.replace('"agent_not_found"', '"not_agent_backed"') + original[end:]
    require(subprocess.check_output(['git', 'show', f'{args.branch}:src/app/api.rs'], cwd=checkout) == expected.encode(), 'unexpected production-source change')
    print(json.dumps({'source': 'pass', 'commit': tip, 'baseline': baseline, 'fixes': commits}))



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
    documents = ['README.md', 'docs/INSTALL.md', 'contracts/README.md', 'AGENTS.md',
                 'macos/Resources/THIRD_PARTY_NOTICES/herdr-APACHE-2.0.txt']
    with tempfile.TemporaryDirectory(prefix='herdr-doc-bump-') as directory:
        fixture = Path(directory)
        for relative in documents + ['scripts/bump-herdr.sh', 'contracts/herdr-api.schema.json', str(MANIFEST.relative_to(ROOT))]:
            target = fixture / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        run('zsh', 'scripts/bump-herdr.sh', '--repo', 'herdrdev/herdr', 'v0.8.2', cwd=fixture)
        for relative in documents:
            text = (fixture / relative).read_text()
            require('modified Herdr preview' not in text and pin['tag'].rsplit('-', 1)[-1] not in text, f'stale fork provenance in {relative}')
            require('not modified by hide' in text, f'upstream provenance missing in {relative}')
        run('zsh', 'scripts/bump-herdr.sh', '--repo', pin['repo'], pin['tag'], cwd=fixture)
        for relative in documents:
            require((fixture / relative).read_text() == (ROOT / relative).read_text(), f'provenance round trip differs in {relative}')
    print(json.dumps({'bump': 'pass', 'current': unchanged, 'upstream': planned, 'provenance_round_trip': 'pass'}))


def workflow(args):
    text = (ROOT / '.github/workflows/herdr-update.yml').read_text()
    require('gh api repos/herdrdev/herdr/releases/latest' in text, 'upstream stable polling missing')
    require('zsh scripts/bump-herdr.sh --repo herdrdev/herdr "${{ steps.upstream.outputs.latest_tag }}"' in text, 'upstream repository override missing')
    print(json.dumps({'workflow': 'pass'}))


def proposal(args):
    pin = json.loads(MANIFEST.read_text())
    remote = public_api(f'repos/{pin["repo"]}/branches/upstream-proposal')
    require(remote['commit']['sha'] == '13d8d0b99033e6855ce66bc0f96654615c8a17a6', 'proposal tip differs from tested fallback')
    print(json.dumps({'proposal': 'pass', 'commit': remote['commit']['sha'], 'rebase': 'aborted after conflicts; tested pre-rebase source retained'}))


def discussion(args):
    record = Path(run('git', 'rev-parse', '--path-format=absolute', '--git-common-dir')).parent
    draft = record / 'agents/runs/herdr-runtime-release/upstream-discussion.md'
    text = draft.read_text()
    require(len(text.split()) >= 100 and 'https://github.com/modakbul-gongbang/herdr/tree/upstream-proposal' in text, 'discussion draft incomplete')
    print(text)


def attribution(args):
    pin = json.loads(MANIFEST.read_text())
    release = public_api(f'repos/{pin["repo"]}/releases/tags/{pin["tag"]}')
    texts = [run('git', 'log', '--format=%B', 'a90f61b..HEAD'),
             run('git', 'branch', '--show-current'), release['body'], release['name']]
    for branch in ['hide-runtime', 'upstream-proposal']:
        texts.append(branch)
        commits = public_api(f'repos/{pin["repo"]}/compare/1e107419...{branch}')
        texts.extend(c['commit']['message'] for c in commits['commits'])
    record = Path(run('git', 'rev-parse', '--path-format=absolute', '--git-common-dir')).parent
    body = record / 'agents/runs/herdr-runtime-release/delivery/pr-body.md'
    if body.exists(): texts.append(body.read_text())
    pattern = r'(?i)(?:co-authored-by:.*(?:claude|codex|openai|anthropic)|generated (?:by|with) (?:claude|codex|chatgpt)|implemented by (?:claude|codex))'
    require(not any(re.search(pattern, text) for text in texts), 'attribution found')
    print(json.dumps({'attribution': 'pass', 'surfaces': len(texts), 'pr_draft_present': body.exists()}))


parser = argparse.ArgumentParser(description=__doc__)
subs = parser.add_subparsers(dest='command', required=True)
p = subs.add_parser('source'); p.add_argument('--checkout', default=str(Path(run('git', 'rev-parse', '--path-format=absolute', '--git-common-dir')).parent.parent / 'herdr')); p.add_argument('--repo', required=True); p.add_argument('--branch', default='hide-runtime'); p.set_defaults(fn=source)
p = subs.add_parser('asset'); p.add_argument('--reference-binary', default=str(Path(pwd.getpwuid(os.getuid()).pw_dir) / '.local/bin/herdr')); p.set_defaults(fn=asset)
p = subs.add_parser('bump'); p.set_defaults(fn=bump)
p = subs.add_parser('workflow'); p.set_defaults(fn=workflow)
p = subs.add_parser('proposal'); p.set_defaults(fn=proposal)
p = subs.add_parser('discussion'); p.set_defaults(fn=discussion)
p = subs.add_parser('attribution'); p.set_defaults(fn=attribution)
args = parser.parse_args(); args.fn(args)
