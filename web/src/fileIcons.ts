// The Seti mark a file row draws, resolved from the name alone, mirroring
// macos/Sources/HerdrMacOS/SetiFileIcon.swift so the web outline shows the
// same glyph as the Swift one. Colours are the tokens the design system owns,
// never literals: scripts/check-web-tokens.mjs refuses a hex colour in web/src.
//
// The font is the subset the app already ships (MIT, notice at
// macos/Resources/THIRD_PARTY_NOTICES/seti-ui-MIT.txt), copied byte for byte
// to web/src/assets/seti-subset.woff and loaded as "seti" by index.css. The
// codepoints below are the private-use range that subset carries, and
// SetiFileIconTests.swift is the Swift half of the same coverage check.

type FileColor =
  | "text-file-neutral"
  | "text-file-document"
  | "text-file-blue"
  | "text-file-green"
  | "text-file-orange"
  | "text-file-yellow"
  | "text-file-purple";

export type FileIcon = {
  /** The private-use codepoint the seti subset draws. */
  glyph: string;
  /** The class the --color-file-* token backs, so no row writes a colour. */
  color: FileColor;
};

const NEUTRAL = "text-file-neutral";
const DOCUMENT = "text-file-document";
const BLUE = "text-file-blue";
const GREEN = "text-file-green";
const ORANGE = "text-file-orange";
const YELLOW = "text-file-yellow";
const PURPLE = "text-file-purple";

/** The one deliberate generic: a plain document, at the same size and colour
 * as every other row rather than a missing glyph or an empty cell. */
const FALLBACK: FileIcon = { glyph: "\ue023", color: DOCUMENT };

function table(names: string[], icon: FileIcon): Record<string, FileIcon> {
  return Object.fromEntries(names.map((name) => [name, icon]));
}

// Whole names first: a name carrying no extension, or nothing but a leading
// dot, never reaches the extension table.
const NAMED: Record<string, FileIcon> = {
  ...table(
    [".gitignore", ".gitattributes", ".gitmodules", ".gitconfig", ".gitkeep", ".mailmap"],
    { glyph: "\ue034", color: NEUTRAL },
  ),
  ...table([".dockerignore"], { glyph: "\ue025", color: BLUE }),
  ...table(
    ["makefile", "gnumakefile", "cmakelists.txt", "justfile", "rakefile", "gemfile", "procfile", "brewfile"],
    { glyph: "\ue05f", color: ORANGE },
  ),
  ...table(
    [
      ".zshrc", ".zshenv", ".zprofile", ".zlogin", ".zlogout",
      ".bashrc", ".bash_profile", ".bash_logout", ".profile",
      ".inputrc", ".hushlogin", ".envrc",
    ],
    { glyph: "\ue089", color: GREEN },
  ),
  ...table(
    [
      ".editorconfig", ".npmrc", ".nvmrc", ".yarnrc", ".tool-versions",
      ".ruby-version", ".node-version", ".python-version",
      ".npmignore", ".eslintignore", ".prettierignore", ".swiftformat",
    ],
    { glyph: "\ue019", color: NEUTRAL },
  ),
  ...table(["package-lock.json", "yarn.lock", "pnpm-lock.yaml"], { glyph: "\ue05d", color: GREEN }),
};

const LICENSE_PREFIXES = ["license", "licence", "copying"];

const EXTENSIONS: Record<string, FileIcon> = {
  swift: { glyph: "\ue092", color: ORANGE },
  rs: { glyph: "\ue082", color: NEUTRAL },
  ts: { glyph: "\ue099", color: BLUE },
  tsx: { glyph: "\ue07d", color: BLUE },
  jsx: { glyph: "\ue07d", color: BLUE },
  js: { glyph: "\ue051", color: YELLOW },
  mjs: { glyph: "\ue051", color: YELLOW },
  cjs: { glyph: "\ue051", color: YELLOW },
  md: { glyph: "\ue060", color: BLUE },
  markdown: { glyph: "\ue060", color: BLUE },
  json: { glyph: "\ue055", color: YELLOW },
  jsonc: { glyph: "\ue055", color: YELLOW },
  jsonl: { glyph: "\ue055", color: YELLOW },
  toml: { glyph: "\ue019", color: NEUTRAL },
  ini: { glyph: "\ue019", color: NEUTRAL },
  cfg: { glyph: "\ue019", color: NEUTRAL },
  conf: { glyph: "\ue019", color: NEUTRAL },
  config: { glyph: "\ue019", color: NEUTRAL },
  env: { glyph: "\ue019", color: NEUTRAL },
  plist: { glyph: "\ue019", color: NEUTRAL },
  yaml: { glyph: "\ue0a7", color: PURPLE },
  yml: { glyph: "\ue0a7", color: PURPLE },
  sh: { glyph: "\ue089", color: GREEN },
  bash: { glyph: "\ue089", color: GREEN },
  zsh: { glyph: "\ue089", color: GREEN },
  fish: { glyph: "\ue089", color: GREEN },
  py: { glyph: "\ue07b", color: BLUE },
  pyw: { glyph: "\ue07b", color: BLUE },
  png: { glyph: "\ue04c", color: PURPLE },
  jpg: { glyph: "\ue04c", color: PURPLE },
  jpeg: { glyph: "\ue04c", color: PURPLE },
  gif: { glyph: "\ue04c", color: PURPLE },
  webp: { glyph: "\ue04c", color: PURPLE },
  tiff: { glyph: "\ue04c", color: PURPLE },
  heic: { glyph: "\ue04c", color: PURPLE },
  avif: { glyph: "\ue04c", color: PURPLE },
  svg: { glyph: "\ue091", color: PURPLE },
  html: { glyph: "\ue048", color: ORANGE },
  htm: { glyph: "\ue048", color: ORANGE },
  css: { glyph: "\ue01d", color: BLUE },
  scss: { glyph: "\ue01d", color: BLUE },
  sass: { glyph: "\ue01d", color: BLUE },
  less: { glyph: "\ue01d", color: BLUE },
  txt: { glyph: "\ue023", color: DOCUMENT },
  log: { glyph: "\ue023", color: DOCUMENT },
};

/** The extension Foundation would report, which is what the Swift catalog
 * reads: a leading dot is not an extension, so .env resolves as a name. */
function extensionOf(lowercased: string): string {
  const dot = lowercased.lastIndexOf(".");
  if (dot <= 0 || dot === lowercased.length - 1) return "";
  return lowercased.slice(dot + 1);
}

export function fileIcon(name: string): FileIcon {
  const lowercased = name.toLowerCase();
  const named = NAMED[lowercased];
  if (named) return named;
  if (lowercased.startsWith("readme")) return { glyph: "\ue04d", color: BLUE };
  if (LICENSE_PREFIXES.some((prefix) => lowercased.startsWith(prefix))) return { glyph: "\ue05a", color: YELLOW };
  if (lowercased.startsWith("dockerfile")) return { glyph: "\ue025", color: BLUE };
  if (lowercased.startsWith(".env")) return { glyph: "\ue019", color: NEUTRAL };
  // A dot-name ending in rc is a tool's run-control file whatever the tool is,
  // so the class is claimed rather than each new tool's name.
  if (lowercased.startsWith(".") && lowercased.endsWith("rc")) return { glyph: "\ue019", color: NEUTRAL };
  if (lowercased.endsWith(".lock")) return { glyph: "\ue05d", color: GREEN };
  return EXTENSIONS[extensionOf(lowercased)] ?? FALLBACK;
}

export { FALLBACK as fallbackFileIcon };
