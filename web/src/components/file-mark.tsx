// A file or folder's mark (fileIcons.ts). The picture is a background so one
// element serves both themes: index.css swaps in the light variant under the
// light theme, and the asset is fetched only once the mark is drawn.

import type { CSSProperties } from "react";
import type { FileIcon } from "../fileIcons";
import { cn } from "../lib/utils";

export function FileMark({ icon, className, ...data }: { icon: FileIcon; className?: string } & Record<`data-${string}`, string>) {
  const style = { "--file-mark": `url("${icon.url}")`, "--file-mark-light": `url("${icon.lightUrl}")` } as CSSProperties;
  return <span aria-hidden="true" data-file-mark="" className={cn("size-(--size-icon) shrink-0", className)} style={style} {...data} />;
}
