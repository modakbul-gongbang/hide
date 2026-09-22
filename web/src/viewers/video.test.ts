import { describe, expect, it } from "vitest";
import { isVideoPath, videoExtension, videoMime } from "./video";

describe("video files", () => {
  it("names the extension the PRD lists, case-insensitively", () => {
    expect(videoExtension("/repo/clip.mp4")).toBe("mp4");
    expect(videoExtension("/repo/Clip.MOV")).toBe("mov");
    expect(videoExtension("/repo/clip.webm")).toBe("webm");
  });

  it("is not a video for a text, image or extensionless path", () => {
    expect(isVideoPath("/repo/notes.md")).toBe(false);
    expect(isVideoPath("/repo/shot.png")).toBe(false);
    expect(isVideoPath("/repo/LICENSE")).toBe(false);
    expect(isVideoPath("/repo/.mp4")).toBe(false);
  });

  it("maps a playable type and falls back for an unknown one", () => {
    expect(videoMime("/repo/clip.webm")).toBe("video/webm");
    expect(videoMime("/repo/clip.mp4")).toBe("video/mp4");
    expect(videoMime("/repo/notes.md")).toBe("application/octet-stream");
  });
});
