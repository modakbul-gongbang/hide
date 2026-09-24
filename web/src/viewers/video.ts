// Which files play as video (PRD B7, D-09): the core keeps no Video document
// kind, so the web decides from the extension and the bytes it already reads
// for the other viewers. `mov` is included because the PRD names it; the
// browser may still refuse its codec, which the viewer reports in one line.

const MIME: Record<string, string> = {
  mp4: "video/mp4",
  m4v: "video/mp4",
  webm: "video/webm",
  mov: "video/quicktime",
};

export function videoExtension(path: string): string | null {
  // A leading dot is a hidden name, not an extension (`.mp4` is a dotfile),
  // which is the rule the core's own language table uses.
  const dot = path.lastIndexOf(".");
  if (dot <= path.lastIndexOf("/") + 1 || dot === path.length - 1) return null;
  const extension = path.slice(dot + 1).toLowerCase();
  return extension in MIME ? extension : null;
}

export function isVideoPath(path: string): boolean {
  return videoExtension(path) !== null;
}

export function videoMime(path: string): string {
  const extension = videoExtension(path);
  return (extension && MIME[extension]) || "application/octet-stream";
}
