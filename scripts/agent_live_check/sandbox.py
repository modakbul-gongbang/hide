"""Fail-closed macOS write confinement for authenticated native CLI probes.

The operator HOME stays readable for existing authentication. Existing session
files are read-only too: a resume-picker probe cannot modify another session.
Only new histories, the disposable probe tree and its private sockets may be
written. A failed enforcement self-test prevents any provider launch.
"""

import json
import os
from pathlib import Path
import socket
import sys

from .processes import OwnedProcesses
from .protection import MAX_CONFIG_FILES, ProtectionError, beneath, private_directory, write_private


class WriteSandbox:
    def __init__(self, run: Path, sockets: Path, operator_home: Path,
                 histories: list[Path]):
        if sys.platform != "darwin" or not Path("/usr/bin/sandbox-exec").is_file():
            raise ProtectionError("authenticated_probes_require_macos_write_sandbox")
        self.run = run.resolve()
        self.sockets = sockets.resolve()
        self.probe = self.run / "probe"
        self.temp = self.run / "agent-tmp"
        for path in (self.probe, self.temp):
            private_directory(path)
        existing = []
        for root in histories:
            if (not beneath(root, operator_home) or root.is_symlink()
                    or any(p.is_symlink() for p in root.parents
                           if beneath(p, operator_home))):
                raise ProtectionError("history_path_outside_operator_home_or_link")
            pending = [root]
            while pending:
                path = pending.pop()
                if not path.exists() and not path.is_symlink():
                    continue
                existing.append(path.resolve())
                if len(existing) + len(pending) > MAX_CONFIG_FILES:
                    raise ProtectionError("history_inventory_over_budget")
                if path.is_dir() and not path.is_symlink():
                    pending.extend(path.iterdir())
        quote = lambda value: json.dumps(str(value), ensure_ascii=True)
        allowed = [self.probe, self.temp, self.sockets, *[p.resolve() for p in histories]]
        # The remote-unix grammar follows the installed system sandbox profiles.
        rules = ["(version 1)", "(allow default)", "(deny file-write*)",
                 "(allow file-write* " + " ".join(f"(subpath {quote(p)})" for p in allowed) + ")",
                 '(allow file-write* (literal "/dev/null") (literal "/dev/tty"))',
                 '(deny network-outbound (remote unix-socket))',
                 f"(allow network-outbound (remote unix-socket (subpath {quote(self.sockets)})))",
                 f"(allow network-outbound (remote unix-socket (subpath {quote(self.run)})))"]
        for path in existing:
            # A literal protects the existing inode, including directory mode
            # and deletion. New child paths still use the session allowance.
            rules.append(f"(deny file-write* (literal {quote(path)}))")
        for path in (operator_home / ".hide", operator_home / ".config/herdr"):
            rules.append(f"(deny file-read* (subpath {quote(path.resolve())}))")
        self.profile = self.run / "native-write-guard.sb"
        write_private(self.profile, ("\n".join(rules) + "\n").encode())

    def command(self, argv: list[str]) -> list[str]:
        return ["/usr/bin/sandbox-exec", "-f", str(self.profile), *argv]

    def verify(self, owner: OwnedProcesses, env: dict, outside: Path) -> None:
        """Exercise allowed writes, forbidden writes, and Unix socket isolation.

        Both the forbidden file and listening socket belong to this self-test,
        not the operator. No operator endpoint is contacted even if the profile
        is broken. The file is outside all allowed trees and removed on failure.
        """
        forbidden = outside / "must-not-be-created"
        endpoint = outside / "operator-stand-in.sock"
        inside = self.probe / "guard-write-proof"
        if any(path.exists() for path in (forbidden, endpoint, inside)):
            raise ProtectionError("guard_selftest_paths_already_exist")
        listener = socket.socket(socket.AF_UNIX)
        try:
            listener.bind(str(endpoint))
            listener.listen(1)
            program = (
                "import pathlib,socket,sys; "
                f"pathlib.Path({str(inside)!r}).write_bytes(b'proof'); "
                f"p=pathlib.Path({str(forbidden)!r}); "
                "\ntry: p.write_bytes(b'forbidden')"
                "\nexcept PermissionError: pass"
                "\nelse: sys.exit(31)"
                "\ns=socket.socket(socket.AF_UNIX)"
                f"\ntry: s.connect({str(endpoint)!r})"
                "\nexcept PermissionError: pass"
                "\nelse: sys.exit(32)"
            )
            code, _, _ = owner.run(self.command([sys.executable, "-c", program]),
                                   env=env, check=False)
            if code or not inside.exists() or forbidden.exists():
                raise ProtectionError("write_or_socket_guard_not_enforced")
        finally:
            listener.close()
            for path in (forbidden, endpoint, inside):
                path.unlink(missing_ok=True)
