// One file's bytes, read through hided and kept until the path changes. The
// viewers share this: an image, a PDF and a video all ask the same question,
// and a read that fails leaves the reason for the caller to draw in one line.

import { useEffect, useState } from "react";
import { requestFileBytes } from "../fileBytes";

export type FileBytesState =
  | { status: "loading" }
  | { status: "ready"; bytes: Uint8Array }
  | { status: "failed"; reason: string };

export function useFileBytes(path: string): FileBytesState {
  const [state, setState] = useState<FileBytesState>({ status: "loading" });
  useEffect(() => {
    let live = true;
    setState({ status: "loading" });
    requestFileBytes(path)
      .then((bytes) => {
        if (live) setState({ status: "ready", bytes });
      })
      .catch((error: unknown) => {
        if (live) setState({ status: "failed", reason: error instanceof Error ? error.message : "read_failed" });
      });
    return () => {
      live = false;
    };
  }, [path]);
  return state;
}
