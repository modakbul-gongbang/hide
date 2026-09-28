// The first prompt an agent a pull request is handed to starts from.

import { describe, expect, it } from "vitest";
import { delegatePrompt } from "./prDelegate";

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
      }),
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
    expect(delegatePrompt(pr, { failed_checks: [], change_requests: [{ author: "bo", body: "  " }] })).toBe(
      "PR #190 (cargo/tokio-tungstenite-0.29.0)의 CI 실패와 변경 요청을 고쳐줘: Bump tokio-tungstenite",
    );
  });
});
