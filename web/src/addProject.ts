// Add a project's own rules: which hosts the dialog offers, the one refusal
// the shell can tell from the snapshot before an event goes out, the line a
// refusal is shown as, and Clone from URL's reading of a URL and its default
// parent. Everything else about a path hided (this Mac) or the device's
// helper decides and answers, and the core parses a URL again before Git
// sees it (`hide_host::clone`).

import type { Device, WorkspaceRegistration } from "./snapshot";

export type Host = { id: string; label: string };

/** This Mac first, then every registered device, as the Host selector lists them. */
export function addProjectHosts(devices: readonly Device[] | undefined): Host[] {
  const local = devices?.find((device) => device.id === "local");
  return [{ id: "local", label: local?.label ?? "This Mac" }, ...(devices ?? []).filter((device) => device.id !== "local").map((device) => ({ id: device.id, label: device.label }))];
}

/** The host the dialog opens on: the focused device while it is still listed, this Mac otherwise. */
export function initialHost(hosts: readonly Host[], focused: string | null | undefined): string {
  return focused && hosts.some((host) => host.id === focused) ? focused : "local";
}

/** A path without its trailing slashes, so `~/a/` and `~/a` are one folder. */
export function trimFolder(raw: string): string {
  const trimmed = raw.trim();
  return trimmed.length > 1 ? trimmed.replace(/\/+$/, "") : trimmed;
}

/** The folder's last name, which the project is labelled with. */
export function folderLabel(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** True when `path` is already a project on `device`; no event goes out for it. */
export function alreadyRegistered(path: string, device: string, registrations: readonly WorkspaceRegistration[]): boolean {
  return registrations.some((row) => row.device_id === device && trimFolder(row.path) === path);
}

/** A refusal as one line, for a reason code hided answered or the shell's own `already_registered`. */
export function refusalText(reason: string): string {
  switch (reason) {
    case "already_registered":
      return "This folder is already a project.";
    case "outside_home":
      return "Only a folder inside your home folder can be added.";
    case "home_root":
      return "Your home folder itself cannot be a project; pick a folder inside it.";
    case "invalid_path":
      return "The path has to be absolute, without `..`.";
    case "not_found":
      return "This folder does not exist.";
    case "not_a_directory":
      return "This is a file, not a folder.";
    case "target_exists":
      return "A folder with this name is already here; choose another parent folder.";
    default:
      return `The folder was refused (${reason}).`;
  }
}

/**
 * A URL Git can clone and the folder it lands in, read the way the core reads
 * it (`hide_host::clone::CloneSource`): https, ssh, the scp form
 * `[user@]host:path`, and `file:///path`; the folder is the last segment
 * without `.git`. The shell reads it to name the folder and enable Clone; the
 * core decides.
 */
export type CloneUrl = { ok: true; host: string; name: string } | { ok: false; reason: string };

export function parseCloneUrl(raw: string): CloneUrl {
  const url = raw.trim();
  if (!url) return { ok: false, reason: "Enter a Git URL." };
  // `::` is Git's transport-helper form (`ext::`), never a URL this accepts.
  // eslint-disable-next-line no-control-regex
  if (url.startsWith("-") || url.includes("::") || /[\s\u0000-\u001f\u007f]/.test(url)) return { ok: false, reason: "This is not a Git URL." };
  let host: string;
  let path: string;
  const scheme = /^(https|ssh|file):\/\/(.*)$/.exec(url);
  if (scheme) {
    const [, kind, rest] = scheme as unknown as [string, string, string];
    if (kind === "file") {
      if (!rest.startsWith("/")) return { ok: false, reason: "A file URL names an absolute path: file:///path/to/repo.git" };
      host = "localhost";
      path = rest;
    } else {
      const slash = rest.indexOf("/");
      if (slash < 0) return { ok: false, reason: "This URL names no repository to clone." };
      const authority = rest.slice(0, slash);
      const hostPort = authority.slice(authority.lastIndexOf("@") + 1);
      host = hostPort.replace(/:\d*$/, "");
      path = rest.slice(slash + 1);
      if (!validHost(host)) return { ok: false, reason: "This URL names no host." };
    }
  } else if (url.includes("://")) {
    return { ok: false, reason: "Use an https, ssh or git@host:path URL." };
  } else {
    const colon = url.indexOf(":");
    const authority = colon < 0 ? "" : url.slice(0, colon);
    host = authority.slice(authority.lastIndexOf("@") + 1);
    if (colon < 0 || authority.includes("/") || !validHost(host)) return { ok: false, reason: "Use an https, ssh or git@host:path URL." };
    path = url.slice(colon + 1);
  }
  const trimmed = path.replace(/\/+$/, "");
  const stem = (trimmed.endsWith(".git") ? trimmed.slice(0, -4) : trimmed).replace(/\/+$/, "");
  const name = stem.split(/[/:]/).pop() ?? "";
  if (!name || name === "." || name === ".." || name.includes("\\")) return { ok: false, reason: "This URL names no repository to clone." };
  return { ok: true, host: host.toLowerCase(), name };
}

function validHost(host: string): boolean {
  return host.length > 0 && /^[A-Za-z0-9._\-[\]:]+$/.test(host);
}

/** Where a clone goes by default: beside the most recently added project on this Mac, else home. */
export function defaultCloneParent(registrations: readonly WorkspaceRegistration[]): string {
  const last = registrations.filter((row) => row.device_id === "local").at(-1);
  if (!last) return "~";
  const path = trimFolder(last.path);
  const slash = path.lastIndexOf("/");
  return slash > 0 ? path.slice(0, slash) : "~";
}
