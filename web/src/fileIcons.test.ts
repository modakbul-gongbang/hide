import { describe, expect, it } from "vitest";
import { fileIcon, folderIcon } from "./fileIcons";

// The expected pictures are the ones Material Icon Theme's own manifest names
// for each case, read from its documented precedence: a whole file name, then
// the longest extension, then the generic file; a light variant only where the
// theme draws one.
const picture = (url: string) => url.slice(url.lastIndexOf("/") + 1).replace(/\?.*$/, "");

describe("fileIcon", () => {
  it("prefers a whole file name to its extension", () => {
    expect(picture(fileIcon("package.json").url)).toBe("nodejs.svg");
    expect(picture(fileIcon("README.md").url)).toBe("readme.svg");
    expect(picture(fileIcon("notes.md").url)).toBe("markdown.svg");
  });

  it("tries the longest extension first", () => {
    expect(picture(fileIcon("App.test.tsx").url)).toBe("test-jsx.svg");
    expect(picture(fileIcon("snapshot.d.ts").url)).toBe("typescript-def.svg");
    expect(picture(fileIcon("App.tsx").url)).toBe("react_ts.svg");
  });

  it("reads a dot-name's suffix as its extension", () => {
    expect(picture(fileIcon(".zshrc").url)).toBe("console.svg");
  });

  it("matches the last name of a path, whatever its case", () => {
    expect(picture(fileIcon("web/PACKAGE.JSON").url)).toBe("nodejs.svg");
  });

  it("draws the generic file for a name the theme does not know", () => {
    expect(picture(fileIcon("notes.unknown-kind").url)).toBe("file.svg");
    expect(picture(fileIcon("Procfile.").url)).toBe("file.svg");
  });

  it("draws the light variant only where the theme has one", () => {
    expect(picture(fileIcon("Cargo.toml").url)).toBe("toml.svg");
    expect(picture(fileIcon("Cargo.toml").lightUrl)).toBe("toml_light.svg");
    expect(fileIcon("main.rs").lightUrl).toBe(fileIcon("main.rs").url);
  });
});

describe("folderIcon", () => {
  it("draws a named folder closed and open", () => {
    expect(picture(folderIcon("web/src", false).url)).toBe("folder-src.svg");
    expect(picture(folderIcon("web/src", true).url)).toBe("folder-src-open.svg");
    expect(picture(folderIcon(".github", false).url)).toBe("folder-github.svg");
  });

  it("draws the plain folder for a name the theme does not know", () => {
    expect(picture(folderIcon("1002-grabtheworld-profile", false).url)).toBe("folder.svg");
    expect(picture(folderIcon("1002-grabtheworld-profile", true).url)).toBe("folder-open.svg");
  });
});
