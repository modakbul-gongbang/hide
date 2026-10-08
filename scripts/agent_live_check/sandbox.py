"""Fail-closed macOS write confinement for authenticated native CLI probes.

The operator HOME stays readable for existing authentication. Declared history
trees, the disposable probe tree and its private sockets may be written.
Nonshared declared configuration files remain protected, including when an
allowance overlaps them. A failed self-test prevents any provider launch.
"""

import json
import os
from pathlib import Path
import socket
import sys

from .processes import OwnedProcesses
from .protection import ProtectionError, beneath, private_directory, write_private


class WriteSandbox:
    def __init__(self, run: Path, sockets: Path, operator_home: Path,
                 histories: list[Path], state: Path | None = None, *,
                 protected: list[Path] | None = None):
        if sys.platform != "darwin" or not Path("/usr/bin/sandbox-exec").is_file():
            raise ProtectionError("authenticated_probes_require_macos_write_sandbox")
        self.run = run.resolve()
        self.sockets = sockets.resolve()
        self.probe = self.run / "probe"
        self.state = (state or self.run / "state").resolve()
        if not beneath(self.state, self.run):
            raise ProtectionError("sandbox_state_outside_run")
        self.temp = self.run / "agent-tmp"
        protected = protected or []
        for path in (self.probe, self.temp):
            private_directory(path)
        for root in histories:
            if (not beneath(root, operator_home) or root.is_symlink()
                    or any(p.is_symlink() for p in root.parents
                           if beneath(p, operator_home))):
                raise ProtectionError("history_path_outside_operator_home_or_link")
        if any(not beneath(path, operator_home) or path.is_symlink() for path in protected):
            raise ProtectionError("protected_configuration_outside_operator_home_or_link")
        quote = lambda value: json.dumps(str(value), ensure_ascii=True)
        allowed = [self.probe, self.temp, self.sockets, *[p.resolve() for p in histories]]
        # The remote-unix grammar follows the installed system sandbox profiles.
        rules = ["(version 1)", "(allow default)", "(deny file-write*)",
                 "(deny signal (target others))",
                 "(allow signal (target same-sandbox))",
                 "(deny appleevent-send hid-control mach-task*)",
                 "(deny process-info*)",
                 "(allow process-info* (target self))",
                 '(deny sysctl-read (sysctl-name-prefix "kern.proc"))',
                 "(allow file-write* " + " ".join(f"(subpath {quote(p)})" for p in allowed) + ")",
                 '(allow file-write* (literal "/dev/null") (literal "/dev/tty"))',
                 '(deny network-outbound (remote unix-socket))',
                 f"(allow network-outbound (remote unix-socket (subpath {quote(self.sockets)})))",
                 f"(allow network-outbound (remote unix-socket (subpath {quote(self.run)})))",
                 f"(deny file-read* (subpath {quote(self.state)}))"]
        for path in dict.fromkeys(protected):
            rules.append(f"(deny file-write* (literal {quote(path.resolve())}))")
        # Installed hook executables may live in .hide/kit. They remain
        # readable; only operator routing/credentials are concealed.
        for path in (operator_home / ".hide/state", operator_home / ".hide/hcoord",
                     operator_home / ".config/herdr"):
            rules.append(f"(deny file-read* (subpath {quote(path.resolve())}))")
        self.profile = self.run / "native-write-guard.sb"
        self.rules = rules
        self.references = set()
        write_private(self.profile, ("\n".join(rules) + "\n").encode())

    def allow_reference(self, reference: Path) -> None:
        from .protection import stamp
        if (reference.parent != self.state / "pane-capabilities" or reference.is_symlink()
                or stamp(reference) is None or len(self.references) >= 256):
            raise ProtectionError("candidate_capability_reference_refused")
        self.references.add(reference)
        rules = list(self.rules)
        for path in sorted(self.references):
            rules.append(f"(allow file-read* (literal {json.dumps(str(path))}))")
            rules.append(f"(allow file-read* file-write* (literal {json.dumps(str(path.with_suffix('.claimed')))}))")
        replacement = self.profile.with_suffix(".next")
        write_private(replacement, ("\n".join(rules) + "\n").encode())
        replacement.replace(self.profile)

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
        if beneath(outside, self.run) or beneath(outside, self.sockets):
            raise ProtectionError("guard_selftest_outside_aliases_allowance")
        if any(path.exists() for path in (forbidden, endpoint, inside)):
            raise ProtectionError("guard_selftest_paths_already_exist")
        listener = socket.socket(socket.AF_UNIX)
        try:
            listener.bind(str(endpoint))
            listener.listen(1)
            program = (
                "import ctypes,errno,os,pathlib,socket,sys; "
                f"pathlib.Path({str(inside)!r}).write_bytes(b'proof'); "
                f"p=pathlib.Path({str(forbidden)!r}); "
                "\ntry: p.write_bytes(b'forbidden')"
                "\nexcept PermissionError: pass"
                "\nelse: sys.exit(31)"
                "\ns=socket.socket(socket.AF_UNIX)"
                f"\ntry: s.connect({str(endpoint)!r})"
                "\nexcept PermissionError: pass"
                "\nelse: sys.exit(32)"
                f"\ntry: os.kill({os.getpid()}, 0)"
                "\nexcept PermissionError: pass"
                "\nelse: sys.exit(33)"
                "\nlib=ctypes.CDLL(None,use_errno=True)"
                f"\nmib=(ctypes.c_int*3)(1,49,{os.getpid()})"
                "\nsize=ctypes.c_size_t(1024*1024); data=ctypes.create_string_buffer(size.value)"
                "\nif lib.sysctl(mib,3,data,ctypes.byref(size),None,0) != -1 or ctypes.get_errno() != errno.EPERM: sys.exit(34)"
            )
            code, _, _ = owner.run(self.command([sys.executable, "-c", program]),
                                   env=env, check=False)
            if code or not inside.exists() or forbidden.exists():
                raise ProtectionError("native_confinement_guard_not_enforced")
        finally:
            listener.close()
            for path in (forbidden, endpoint, inside):
                path.unlink(missing_ok=True)
