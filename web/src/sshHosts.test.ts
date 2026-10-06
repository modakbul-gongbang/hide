import { describe, expect, it } from "vitest";
import { defaultDeviceName, hostChoice, hostChoices, pickableAliases } from "./sshHosts";
import type { SshHost } from "./snapshot";

const host = (overrides: Partial<SshHost> & { alias: string }): SshHost => ({ address: "ops@lab.example.invalid:22", added_as: null, problem: null, ...overrides });

describe("hostChoice", () => {
  it("lets a Host with an address and no device be chosen", () => {
    expect(hostChoice(host({ alias: "lab" }))).toEqual({ kind: "available", alias: "lab", address: "ops@lab.example.invalid:22" });
  });

  it("dims a Host a device already uses, or one that reaches the same machine, as Added as <name>", () => {
    expect(hostChoice(host({ alias: "studio-alias", added_as: "Studio Mac" }))).toEqual({
      kind: "added",
      alias: "studio-alias",
      address: "ops@lab.example.invalid:22",
      name: "Studio Mac",
    });
  });

  it("keeps Added as even when the address cannot be read", () => {
    expect(hostChoice(host({ alias: "studio", address: null, added_as: "Studio Mac", problem: "ssh_failed" })).kind).toBe("added");
  });

  it("dims a Host whose address could not be read, with the reason its problem code names", () => {
    expect(hostChoice(host({ alias: "a", address: null, problem: "ssh_missing" }))).toEqual({ kind: "unresolved", alias: "a", reason: "devices.hostProblem.sshMissing" });
    expect(hostChoice(host({ alias: "b", address: null, problem: "timed_out" }))).toEqual({ kind: "unresolved", alias: "b", reason: "devices.hostProblem.timedOut" });
    expect(hostChoice(host({ alias: "c", address: null, problem: null }))).toEqual({ kind: "unresolved", alias: "c", reason: "devices.hostProblem.sshFailed" });
  });
});

describe("pickableAliases", () => {
  it("lists only the Hosts that can be chosen, in the order the core listed them", () => {
    const choices = hostChoices([host({ alias: "one" }), host({ alias: "two", added_as: "Two" }), host({ alias: "three", address: null, problem: "ssh_failed" }), host({ alias: "four" })]);
    expect(pickableAliases(choices)).toEqual(["one", "four"]);
  });
});

describe("defaultDeviceName", () => {
  it("fills the alias, which the person can rewrite", () => {
    expect(defaultDeviceName("studio")).toBe("studio");
  });
});
