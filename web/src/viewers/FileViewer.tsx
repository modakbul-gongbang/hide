// The file viewers (PRD B6, B7, D-07, D-09). The core decides the document
// kind when it reads the file; the web adds the video case the core has no
// kind for. Every viewer reads the same way - hided file bytes behind the
// checkout boundary - and every failure is one line in the document's place.

import { useEffect, useRef, useState } from "react";
import { blobUrl } from "../fileBytes";
import { useInterfaceTranslation } from "../i18n/client";
import type { EditorDocumentSnapshot } from "../snapshot";
import { useFileBytes, type FileBytesState } from "./useFileBytes";
import { isVideoPath, videoMime } from "./video";

/** The one line a failed read or an unsupported file leaves behind. */
export function ViewerNotice({ reason, state }: { reason: string; state: string }) {
  return (
    <div className="flex flex-1 items-center justify-center px-md text-center text-caption text-muted-foreground" data-viewer-state={state}>
      {reason}
    </div>
  );
}

export function FileViewer({ document }: { document: EditorDocumentSnapshot }) {
  const { t } = useInterfaceTranslation();
  if (isVideoPath(document.path)) return <VideoView path={document.path} />;
  if (document.document_kind === "image") return <ImageView path={document.path} />;
  if (document.document_kind === "pdf") return <PdfView path={document.path} />;
  return <ViewerNotice state="binary" reason={t("documents.binary")} />;
}

function useBlob(state: FileBytesState, type: string): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (state.status !== "ready") {
      setUrl(null);
      return undefined;
    }
    const next = blobUrl(state.bytes, type);
    setUrl(next);
    return () => URL.revokeObjectURL(next);
  }, [state, type]);
  return url;
}

function ImageView({ path }: { path: string }) {
  const { t } = useInterfaceTranslation();
  const state = useFileBytes(path);
  const url = useBlob(state, "image/*");
  if (state.status === "failed") return <ViewerNotice state="image-failed" reason={t("documents.imageUnavailable", { reason: state.reason })} />;
  if (!url) return <ViewerNotice state="image-loading" reason={t("documents.imageLoading")} />;
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto p-lg" data-viewer="image">
      <img src={url} alt={path} className="max-h-full max-w-full object-contain" />
    </div>
  );
}

function VideoView({ path }: { path: string }) {
  const { t } = useInterfaceTranslation();
  const state = useFileBytes(path);
  const [playbackFailed, setPlaybackFailed] = useState(false);
  const url = useBlob(state, videoMime(path));
  if (state.status === "failed") return <ViewerNotice state="video-failed" reason={t("documents.videoUnavailable", { reason: state.reason })} />;
  if (playbackFailed) return <ViewerNotice state="video-codec" reason={t("documents.videoUnsupported")} />;
  if (!url) return <ViewerNotice state="video-loading" reason={t("documents.videoLoading")} />;
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center p-lg" data-viewer="video">
      <video
        src={url}
        controls
        className="max-h-full max-w-full"
        onError={() => setPlaybackFailed(true)}
      />
    </div>
  );
}

function PdfView({ path }: { path: string }) {
  const { t } = useInterfaceTranslation();
  const state = useFileBytes(path);
  const host = useRef<HTMLDivElement>(null);
  const [renderFailed, setRenderFailed] = useState(false);

  useEffect(() => {
    const container = host.current;
    if (!container || state.status !== "ready") return undefined;
    let cancelled = false;
    let destroy: (() => void) | null = null;
    const render = async () => {
      try {
        const pdfjs = await import("pdfjs-dist");
        const worker = await import("pdfjs-dist/build/pdf.worker.min.mjs?url");
        pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
        // pdf.js detaches the buffer it is handed, so it gets a copy.
        const task = pdfjs.getDocument({ data: state.bytes.slice() });
        destroy = () => {
          void task.destroy();
        };
        const loaded = await task.promise;
        const width = container.clientWidth || 800;
        for (let number = 1; number <= loaded.numPages; number += 1) {
          if (cancelled) return;
          const page = await loaded.getPage(number);
          const base = page.getViewport({ scale: 1 });
          const scale = Math.min(2, Math.max(0.5, (width - 32) / base.width));
          const viewport = page.getViewport({ scale });
          const canvas = document.createElement("canvas");
          canvas.width = viewport.width;
          canvas.height = viewport.height;
          canvas.className = "mx-auto mb-lg shadow-none";
          canvas.setAttribute("data-pdf-page", String(number));
          container.appendChild(canvas);
          const context = canvas.getContext("2d");
          if (context) await page.render({ canvasContext: context, viewport, canvas }).promise;
        }
      } catch {
        if (!cancelled) setRenderFailed(true);
      }
    };
    void render();
    return () => {
      cancelled = true;
      destroy?.();
      container.replaceChildren();
    };
  }, [state]);

  if (state.status === "failed") return <ViewerNotice state="pdf-failed" reason={t("documents.pdfUnavailable", { reason: state.reason })} />;
  if (renderFailed) return <ViewerNotice state="pdf-render" reason={t("documents.pdfRenderFailed")} />;
  if (state.status !== "ready") return <ViewerNotice state="pdf-loading" reason={t("documents.pdfLoading")} />;
  return <div ref={host} className="min-h-0 flex-1 overflow-auto p-lg" data-viewer="pdf" />;
}
