// Add a project's own rules: which hosts the dialog offers, the one refusal
// the shell can tell from the snapshot before an event goes out, and the line
// a refusal is shown as. Everything else about a path hided (this Mac) or the
// device's helper decides and answers.

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
    case "already_exists":
      return "Something by this name is already there.";
    default:
      return `The folder was refused (${reason}).`;
  }
}

/**
 * Where Create new project puts its folder at first: beside the most recently
 * added project on this Mac (registrations are kept in the order they were
 * added), else the home folder.
 */
export function defaultProjectParent(registrations: readonly WorkspaceRegistration[]): string {
  const last = registrations.filter((row) => row.device_id === "local").at(-1);
  if (!last) return "~";
  const path = trimFolder(last.path);
  const cut = path.lastIndexOf("/");
  return cut > 0 ? path.slice(0, cut) : "~";
}

/** Why a typed `name` cannot be a new project's folder name, or null; an empty name has nothing to say yet. */
export function projectNameProblem(name: string): string | null {
  if (name.includes("/")) return "A name is one folder, without `/`.";
  if (name === "." || name === "..") return "`.` and `..` are not folder names.";
  return null;
}

/** The folder a new project takes: its parent and the name, with a placeholder until one is typed. */
export function projectPath(parent: string, name: string): string {
  return `${parent === "/" ? "" : parent}/${name || "project-name"}`;
}
