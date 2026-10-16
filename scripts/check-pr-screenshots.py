#!/usr/bin/env python3
"""Refuse a pull request that changes a screen and shows nothing of it.

CONTRIBUTING.md states the policy; this is its check. Of the twenty pull
requests that touched the web shell on 2026-10-16, seven carried an image: the
rule stood in the template and the agent guide, and every pull request opened
by hand under load skipped it, because nothing refused the omission.

The check asks one thing of a pull request whose diff touches a screen
(`web/src`, `site/` or `design/`, its tests aside): the body shows the screen
in an image a reviewer can open, or says in one line why it does not. The
line is `스크린샷 없음: <reason>` or `No screenshot: <reason>`, and a reviewer
judges the reason; what the check removes is the silent omission.

An image counts only where GitHub renders it for a reviewer: an `https://`
link in `![caption](url)` or `<img src="url">`. A workstation path, a
`file://` link, a relative path and `raw.githubusercontent.com` (private to
the uploader) do not. The upload command is in `AGENTS.md`, Before Opening A
Pull Request.
"""
import argparse
from fnmatch import fnmatchcase
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# What a person looks at: the web shell, the site and the design library.
SCREEN_PATHS = ("web/src/", "site/", "design/")
# Tests and snapshots change no screen.
NOT_A_SCREEN = ("*.test.ts", "*.test.tsx", "*.test.mjs", "*/__snapshots__/*", "web/src/**/test-utils*", "web/src/**/*.d.ts")

# An attribute's quoted value may hold `>`, as an alt such as "Settings > Agents" does.
IMAGE = re.compile(r'!\[[^\]]*\]\(\s*<?([^)\s>]+)>?[^)]*\)|<img\b(?:[^>"\']|"[^"]*"|\'[^\']*\')*?\bsrc\s*=\s*["\']([^"\']+)["\']', re.IGNORECASE)
EXEMPTION = re.compile(r'^\s*(?:스크린샷 없음|No screenshot)\s*:\s*(\S.*)$', re.IGNORECASE | re.MULTILINE)
COMMENT = re.compile(r'<!--.*?-->', re.DOTALL)


def is_screen(path: str) -> bool:
    return path.startswith(SCREEN_PATHS) and not any(fnmatchcase(path, glob) for glob in NOT_A_SCREEN)


def screen_paths(paths) -> list[str]:
    return sorted(path for path in paths if is_screen(path))


def images(body: str) -> list[str]:
    """Every image link in the body, comments removed."""
    return [first or second for first, second in IMAGE.findall(COMMENT.sub('', body))]


def visible(url: str) -> bool:
    return url.startswith('https://') and 'raw.githubusercontent.com' not in url


def exemption(body: str) -> str | None:
    found = EXEMPTION.search(COMMENT.sub('', body))
    return found.group(1).strip() if found else None


def verdict(body: str, paths) -> tuple[bool, str]:
    """(ok, message) for a body and the paths the pull request changes."""
    screens = screen_paths(paths)
    if not screens:
        return True, 'No screen changed; no screenshot needed.'
    shown = images(body)
    hidden = [url for url in shown if not visible(url)]
    if hidden:
        return False, ('a reviewer cannot open: ' + ', '.join(hidden)
                       + '\nUpload the image and link its https:// URL (AGENTS.md, Before Opening A Pull Request).')
    if shown:
        return True, f'{len(shown)} image(s) for {len(screens)} screen path(s).'
    reason = exemption(body)
    if reason is not None:
        return True, f'No screenshot, declared: {reason}'
    listed = '\n'.join(f'  {path}' for path in screens[:12]) + ('\n  …' if len(screens) > 12 else '')
    return False, ('the pull request changes a screen and shows none of it:\n' + listed
                   + '\nPut a screenshot under Summary (AGENTS.md, Before Opening A Pull Request, names the upload command),'
                   ' or write one line `스크린샷 없음: <reason>` / `No screenshot: <reason>` for the reviewer to judge.')


def git(*args: str) -> str:
    return subprocess.check_output(['git', *args], cwd=ROOT, text=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--pr-body', type=Path, required=True, help='path to the pull request body')
    paths = parser.add_mutually_exclusive_group()
    paths.add_argument('--files', type=Path, help='a file naming each changed path on its own line')
    paths.add_argument('--range', default='origin/main...HEAD', help='the commit range to diff (default: origin/main...HEAD)')
    arguments = parser.parse_args()
    if not arguments.pr_body.is_file():
        print(f'error: no pull request body at {arguments.pr_body}', file=sys.stderr)
        return 2
    if arguments.files is not None:
        changed = [line.strip() for line in arguments.files.read_text().splitlines() if line.strip()]
    else:
        changed = git('diff', '--name-only', arguments.range).split()
    ok, message = verdict(arguments.pr_body.read_text(), changed)
    if ok:
        print(f'Screenshots: {message}')
        return 0
    print(f'error: {message}', file=sys.stderr)
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
