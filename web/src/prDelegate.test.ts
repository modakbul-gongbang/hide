// The first prompt an agent a pull request is handed to starts from.

import { describe, expect, it } from "vitest";
import { initializeInterfaceI18n } from "./i18n/instance";
import { delegatePrompt } from "./prDelegate";

const t = initializeInterfaceI18n("ko").getFixedT(null, "translation");
const english = initializeInterfaceI18n("en").getFixedT(null, "translation");

const pr = { number: 190, title: "Bump tokio-tungstenite", branch: "cargo/tokio-tungstenite-0.29.0" };

describe("the prompt a pull request is handed on with", () => {
  it("names the pull request, then each failed check with its link and each change request as written", () => {
    expect(
      delegatePrompt(pr, {
        failed_checks: [
          { name: "verify", url: "https://github.com/acme/app/actions/runs/1" },
          { name: "ci/legacy", url: null },
        ],
        change_requests: [
          { author: "ana", body: "Split the reader.\nThen retry." },
          { author: null, body: "Rename it." },
        ],
      }, t),
    ).toBe(
      [
        "PR #190 (cargo/tokio-tungstenite-0.29.0)의 CI 실패와 변경 요청을 고쳐줘: Bump tokio-tungstenite",
        "",
        "실패한 검사:",
        "- verify https://github.com/acme/app/actions/runs/1",
        "- ci/legacy",
        "",
        "변경 요청:",
        "- ana: Split the reader.\nThen retry.",
        "- 리뷰어: Rename it.",
      ].join("\n"),
    );
  });

  it("leaves out a section with nothing in it", () => {
    expect(delegatePrompt(pr, { failed_checks: [], change_requests: [{ author: "bo", body: "  " }] }, t)).toBe(
      "PR #190 (cargo/tokio-tungstenite-0.29.0)의 CI 실패와 변경 요청을 고쳐줘: Bump tokio-tungstenite",
    );
  });

  it("is written in the interface language, with names and links as data", () => {
    expect(delegatePrompt(pr, { failed_checks: [{ name: "verify", url: null }], change_requests: [{ author: null, body: "Rename it." }] }, english)).toBe(
      [
        "Fix the failed CI and change requests of PR #190 (cargo/tokio-tungstenite-0.29.0): Bump tokio-tungstenite",
        "",
        "Failed checks:",
        "- verify",
        "",
        "Change requests:",
        "- reviewer: Rename it.",
      ].join("\n"),
    );
  });
});
