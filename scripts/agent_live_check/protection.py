"""Pre-launch isolation and private, conflict-preserving configuration recovery."""

from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import stat

MAX_CONFIG_FILES = 50_000
MAX_CONFIG_BYTES = 256 * 1024 * 1024
MAX_BACKUP_BYTES = 16 * 1024 * 1024


class ProtectionError(RuntimeError):
    """A guard failed before permission to launch or restore was established."""


def beneath(path: Path, root: Path) -> bool:
    return path.resolve().is_relative_to(root.resolve())


def private_directory(path: Path) -> None:
    """Create every missing component privately; never widen existing access."""
    pending, parent = [], path
    while not parent.exists():
        if parent.is_symlink():
            raise ProtectionError("private_directory_is_link")
        pending.append(parent)
        parent = parent.parent
    if parent.is_symlink() or path.is_symlink():
        raise ProtectionError("private_directory_is_link")
    for folder in reversed(pending):
        folder.mkdir(mode=0o700)
    mode = path.stat().st_mode
    if path.stat().st_uid != os.getuid() or mode & 0o077:
        raise ProtectionError("private_directory_not_private")


def write_private(path: Path, data: bytes) -> None:
    """Create a new private file, never follow or replace a preexisting name."""
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())


def validate_isolation(run: Path, home: Path, socket: Path, state: Path,
                       operator_home: Path, operator_socket: Path | None,
                       socket_root: Path | None = None) -> None:
    """Refuse operator routing before even starting a version/auth subprocess."""
    if not all(path.is_absolute() for path in (run, home, socket, state)):
        raise ProtectionError("isolation_paths_must_be_absolute")
    if home.resolve() == operator_home.resolve():
        raise ProtectionError("daemon_home_is_operator_home")
    for path in (home, state):
        if not beneath(path, run) or path.resolve() == run.resolve():
            raise ProtectionError("runtime_path_outside_run")
    defaults = [operator_home / ".config/herdr/herdr.sock",
                operator_home / ".hide/state"]
    if operator_socket is not None:
        defaults.append(operator_socket)
    if any(socket.resolve() == path.resolve() for path in defaults):
        raise ProtectionError("operator_socket_refused")
    if socket.exists() or socket.is_symlink():
        raise ProtectionError("socket_already_exists")
    if len(os.fsencode(socket)) > 85:
        raise ProtectionError("socket_path_too_long")
    # Only the run's root or its owned short socket directory may contain it.
    parent = socket.parent
    if not beneath(socket, run):
        if (socket_root is None or parent != socket_root
                or not parent.is_dir() or parent.is_symlink()
                or parent.stat().st_uid != os.getuid()
                or parent.stat().st_mode & 0o077):
            raise ProtectionError("socket_parent_not_owned_by_run")
    for forbidden in (operator_home / ".hide", operator_home / ".config/herdr"):
        if beneath(state, forbidden) or beneath(socket, forbidden):
            raise ProtectionError("operator_state_refused")


@dataclass(frozen=True)
class FileStamp:
    digest: str
    size: int
    mode: int
    identity: tuple[int, int]


def configuration_bytes(path: Path) -> tuple[FileStamp, bytes] | None:
    """Read and stamp one bounded ordinary file through the same descriptor."""
    try:
        info = path.lstat()
    except FileNotFoundError:
        return None
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1:
        raise ProtectionError("config_not_private_regular_file")
    if info.st_size > MAX_BACKUP_BYTES:
        raise ProtectionError("config_file_over_budget")
    # A regular pathname can become a FIFO between lstat and open. Do not
    # block on that replacement while trying to inspect its descriptor.
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        opened = os.fstat(stream.fileno())
        if (not stat.S_ISREG(opened.st_mode) or opened.st_uid != os.getuid()
                or opened.st_nlink != 1
                or (opened.st_dev, opened.st_ino) != (info.st_dev, info.st_ino)):
            raise ProtectionError("config_changed_during_open")
        if opened.st_size > MAX_BACKUP_BYTES:
            raise ProtectionError("config_file_over_budget")
        data = stream.read(MAX_BACKUP_BYTES + 1)
        after = os.fstat(stream.fileno())
    if (len(data) > MAX_BACKUP_BYTES
            or (opened.st_size, opened.st_mtime_ns, opened.st_ctime_ns, opened.st_mode, opened.st_nlink)
            != (after.st_size, after.st_mtime_ns, after.st_ctime_ns, after.st_mode, after.st_nlink)):
        raise ProtectionError("config_changed_during_read")
    return (FileStamp(hashlib.sha256(data).hexdigest(), len(data),
                      stat.S_IMODE(opened.st_mode), (opened.st_dev, opened.st_ino)), data)


def stamp(path: Path) -> FileStamp | None:
    """Hash ordinary owned files; reject aliases and unbounded reads."""
    value = configuration_bytes(path)
    return value[0] if value is not None else None


def fingerprint(roots: list[Path], histories: list[Path] | None = None) -> dict[str, dict]:
    """Inventory whole config trees. Link targets are never traversed or read."""
    result = {}
    histories = histories or []
    total = 0
    pending = list(roots)
    while pending:
        path = pending.pop()
        if len(result) + len(pending) > MAX_CONFIG_FILES:
            raise ProtectionError("config_tree_file_budget")
        try:
            info = path.lstat()
        except FileNotFoundError:
            continue
        name = str(path)
        if stat.S_ISLNK(info.st_mode):
            result[name] = {"kind": "link", "digest": hashlib.sha256(os.fsencode(os.readlink(path))).hexdigest()}
        elif stat.S_ISDIR(info.st_mode):
            # Count directory entries as well as files, including empty trees.
            result[name] = {"kind": "directory", "mode": stat.S_IMODE(info.st_mode)}
            with os.scandir(path) as entries:
                for entry in entries:
                    pending.append(Path(entry.path))
                    if len(result) + len(pending) > MAX_CONFIG_FILES:
                        raise ProtectionError("config_tree_file_budget")
        elif stat.S_ISREG(info.st_mode):
            if info.st_size > MAX_BACKUP_BYTES or any(beneath(path, history) for history in histories):
                # History contents may be large and contain private prompts.
                # Existing histories are immutable in the native sandbox; the
                # full tree inventory records retained additions and changes.
                result[name] = {"kind": "metadata", "size": info.st_size,
                                "mtime_ns": info.st_mtime_ns,
                                "ctime_ns": info.st_ctime_ns,
                                "mode": stat.S_IMODE(info.st_mode),
                                "identity": [info.st_dev, info.st_ino]}
                continue
            total += info.st_size
            if total > MAX_CONFIG_BYTES:
                raise ProtectionError("config_tree_byte_budget")
            value = stamp(path)
            if value is None:
                raise ProtectionError("config_disappeared_during_inventory")
            result[name] = {"kind": "file", "digest": value.digest,
                            "size": value.size, "mode": value.mode}
        else:
            result[name] = {"kind": "other"}
    return result


class ConfigGuard:
    """Backups plus explicit mutation ownership; observing a diff is not ownership.

    The live write sandbox prevents config writes. A caller that deliberately
    performs a reversible, owned write must register its exact before/after
    stamps with record_write. Unknown changes fail recovery without overwriting
    the operator. Backups never contain a public report or log field.
    """

    def __init__(self, backup: Path, known: list[Path], roots: list[Path], *,
                 histories: list[Path] | None = None,
                 exclusive_root: Path | None = None):
        private_directory(backup)
        self.backup = backup
        self.known = list(dict.fromkeys(known))
        self.roots = roots
        self.histories = histories or []
        self.exclusive_root = exclusive_root
        if exclusive_root is not None and (not beneath(exclusive_root, backup.parent)
                                           or exclusive_root == backup.parent):
            raise ProtectionError("exclusive_recovery_root_must_be_owned_by_run")
        self.before = {}
        self.writes = {}
        self.inventory = fingerprint(roots, self.histories)
        for index, path in enumerate(self.known):
            value = configuration_bytes(path)
            before = value[0] if value is not None else None
            self.before[path] = before
            if value is not None:
                write_private(backup / str(index), value[1])
        write_private(backup / "index.json", json.dumps([
            {"path": str(path), "backup": str(index),
             "existed": self.before[path] is not None}
            for index, path in enumerate(self.known)
        ]).encode())

    def record_write(self, path: Path, before: FileStamp | None,
                     after: FileStamp | None) -> None:
        """Called by the owner of a write, never by a before/after scan."""
        # Filesystem rename has no conditional compare-and-swap against an
        # unrelated writer. Only a sole-owned disposable HOME may opt into
        # restoration. Live operator writes are denied by the OS sandbox, and
        # an unexpected operator change is preserved and reported as failure.
        if self.exclusive_root is None or not beneath(path, self.exclusive_root):
            raise ProtectionError("operator_configuration_has_no_exclusive_writer")
        if path not in self.before or before != self.before[path]:
            raise ProtectionError("write_has_no_original_ownership")
        if after != stamp(path):
            raise ProtectionError("write_already_changed")
        self.writes[path] = after

    def finish(self) -> dict:
        changes = []
        failures = []

        def recover(index, path):
            original = self.before[path]
            current = stamp(path)
            if current == original:
                return
            if path not in self.writes or current != self.writes[path]:
                failures.append({"path": str(path), "reason": "unattributed_or_concurrent_change_preserved"})
                return
            # No replace is authorized merely because its bytes happen to match.
            # The owner has ended its writer before calling finish. A competing
            # writer after this check is outside the available filesystem CAS.
            if current != stamp(path):
                failures.append({"path": str(path), "reason": "concurrent_change_preserved"})
                return
            if original is None:
                if current is not None:
                    path.unlink()
            else:
                saved = configuration_bytes(self.backup / str(index))
                if saved is None or saved[0].digest != original.digest:
                    raise ProtectionError("backup_integrity_failed")
                data = saved[1]
                temporary = path.with_name(path.name + ".live-check-restore")
                write_private(temporary, data)
                try:
                    os.chmod(temporary, original.mode)
                    if current != stamp(path):
                        failures.append({"path": str(path), "reason": "concurrent_change_preserved"})
                        return
                    os.replace(temporary, path)
                finally:
                    temporary.unlink(missing_ok=True)
            restored = stamp(path)
            if (restored is None) != (original is None) or (restored and original and (restored.digest, restored.mode) != (original.digest, original.mode)):
                failures.append({"path": str(path), "reason": "restore_failed"})
            else:
                changes.append({"path": str(path), "result": "restored"})
        for index, path in enumerate(self.known):
            try:
                recover(index, path)
            except (OSError, ProtectionError) as error:
                # An alias, unreadable file or failed restore names its
                # subject and cannot skip the other safe comparisons.
                failures.append({"path": str(path), "reason": str(error)})
        after = fingerprint(self.roots, self.histories)
        directory_changes = [{"path": key, "kind": "added" if key not in self.inventory else "removed" if key not in after else "changed"}
                             for key in sorted(self.inventory.keys() | after.keys())
                             if self.inventory.get(key) != after.get(key)]
        known_names = {str(path) for path in self.known}
        for change in directory_changes:
            path = Path(change["path"])
            if (change["path"] not in known_names
                    and not any(beneath(path, history) for history in self.histories)):
                failures.append({"path": change["path"], "reason": "configuration_tree_change_preserved"})
        return {"restored": changes, "failures": failures,
                "directory_changes": directory_changes}
