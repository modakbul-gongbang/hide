// The file viewers (PRD B6, B7, D-07, D-09). The core decides the document
// kind when it reads the file; the web adds the video case the core has no
// kind for. Every viewer reads the same way - hided file bytes behind the
// checkout boundary - and every failure is one line in the document's place.

import { useEffect, useMemo, useRef, useState, type PointerEvent } from "react";
import type { PDFPageProxy } from "pdfjs-dist";
import { blobUrl } from "../fileBytes";
import { useInterfaceTranslation } from "../i18n/client";
import type { EditorDocumentSnapshot } from "../snapshot";
import { useShellStore } from "../store";
import { PdfPages } from "./pdfPages";
import { useFileBytes, type FileBytesState } from "./useFileBytes";
import { useViewerZoom, type LayoutAt } from "./useViewerZoom";
import { isVideoPath, videoMime } from "./video";
import { columnLayout, imageFit, pdfFit, type Size } from "./viewerZoom";

/** The one line a failed read or an unsupported file leaves behind. */
export function ViewerNotice({ reason, state }: { reason: string; state: string }) {
  return (
    <div className="flex flex-1 items-center justify-center px-md text-center text-caption text-muted-foreground" data-viewer-state={state}>
      {reason}
    </div>
  );
}

/**
 * One display's viewer. An image or a PDF zooms like a browser page
 * (`viewerZoom.ts`); each file starts at its fit, so a preview retargeted to
 * another file does not carry the last one's zoom.
 */
export function FileViewer({ document, displayId }: { document: EditorDocumentSnapshot; displayId: string }) {
  const { t } = useInterfaceTranslation();
  if (isVideoPath(document.path)) return <VideoView path={document.path} />;
  if (document.document_kind === "image") return <ImageView key={document.path} path={document.path} displayId={displayId} />;
  if (document.document_kind === "pdf") return <PdfView key={document.path} path={document.path} displayId={displayId} />;
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

function ImageView({ path, displayId }: { path: string; displayId: string }) {
  const { t } = useInterfaceTranslation();
  const state = useFileBytes(path);
  const url = useBlob(state, "image/*");
  const [natural, setNatural] = useState<Size | null>(null);
  const [decodeFailed, setDecodeFailed] = useState(false);
  const layoutAt = useMemo<LayoutAt>(
    () => (natural ? (zoom, viewport) => columnLayout([scaled(natural, imageFit(natural, viewport) * zoom)], viewport, "center") : null),
    [natural],
  );
  const { zoom, layout, viewport, scroller, scrollerRef } = useViewerZoom(displayId, layoutAt);
  const box = layout?.boxes[0];
  const pannable = !!layout && !!viewport && (layout.width > viewport.width || layout.height > viewport.height);
  const pan = usePan(scroller, pannable);
  if (state.status === "failed") return <ViewerNotice state="image-failed" reason={t("documents.imageUnavailable", { reason: state.reason })} />;
  if (decodeFailed) return <ViewerNotice state="image-failed" reason={t("documents.imageUnavailable", { reason: "decode_failed" })} />;
  if (!url) return <ViewerNotice state="image-loading" reason={t("documents.imageLoading")} />;
  return (
    <div
      ref={scrollerRef}
      className={`relative min-h-0 flex-1 overflow-auto [overflow-anchor:none] [scrollbar-gutter:stable] ${pannable ? (pan.dragging ? "cursor-grabbing" : "cursor-grab") : ""}`}
      data-viewer="image"
      data-viewer-zoom={zoom}
      {...pan.handlers}
    >
      <div className="relative" style={layout ? { width: layout.width, height: layout.height } : undefined}>
        <img
          src={url}
          alt={path}
          draggable={false}
          className="absolute select-none"
          // Preflight caps an image at its container's width, and `max-w-none`
          // is the zero spacing token here, so the drawn size is lifted inline.
          style={box ? { left: box.left, top: box.top, width: box.width, height: box.height, maxWidth: "none" } : { visibility: "hidden" }}
          onLoad={(event) => {
            const image = event.currentTarget;
            if (image.naturalWidth > 0 && image.naturalHeight > 0) setNatural({ width: image.naturalWidth, height: image.naturalHeight });
            else setDecodeFailed(true);
          }}
          onError={() => setDecodeFailed(true)}
        />
      </div>
    </div>
  );
}

function scaled(size: Size, scale: number): Size {
  return { width: size.width * scale, height: size.height * scale };
}

/** Dragging a zoomed image moves it under the pointer, as a hand tool does. */
function usePan(scroller: HTMLElement | null, pannable: boolean) {
  const [dragging, setDragging] = useState(false);
  const from = useRef<{ pointer: number; x: number; y: number; left: number; top: number } | null>(null);
  const end = (event: PointerEvent<HTMLElement>) => {
    if (from.current?.pointer !== event.pointerId) return;
    from.current = null;
    setDragging(false);
  };
  return {
    dragging,
    handlers: {
      onPointerDown: (event: PointerEvent<HTMLElement>) => {
        // A press on the scroller itself is on its scroll bar.
        if (!pannable || !scroller || event.button !== 0 || event.target === event.currentTarget) return;
        from.current = { pointer: event.pointerId, x: event.clientX, y: event.clientY, left: scroller.scrollLeft, top: scroller.scrollTop };
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      },
      onPointerMove: (event: PointerEvent<HTMLElement>) => {
        const start = from.current;
        if (!start || start.pointer !== event.pointerId || !scroller) return;
        scroller.scrollLeft = start.left - (event.clientX - start.x);
        scroller.scrollTop = start.top - (event.clientY - start.y);
      },
      onPointerUp: end,
      onPointerCancel: end,
    },
  };
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

type PdfDocument = { pages: PDFPageProxy[]; sizes: Size[] };

function PdfView({ path, displayId }: { path: string; displayId: string }) {
  const { t } = useInterfaceTranslation();
  const state = useFileBytes(path);
  const [pdf, setPdf] = useState<PdfDocument | null>(null);
  const [renderFailed, setRenderFailed] = useState(false);

  useEffect(() => {
    if (state.status !== "ready") return undefined;
    let cancelled = false;
    let destroy: (() => void) | null = null;
    const load = async () => {
      try {
        const pdfjs = await import("pdfjs-dist");
        const worker = await import("pdfjs-dist/build/pdf.worker.min.mjs?url");
        pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
        // pdf.js detaches the buffer it is handed, so it gets a copy.
        const task = pdfjs.getDocument({ data: state.bytes.slice() });
        destroy = () => {
          void task.destroy();
        };
        if (cancelled) return destroy();
        const loaded = await task.promise;
        const pages = await Promise.all(Array.from({ length: loaded.numPages }, (_, index) => loaded.getPage(index + 1)));
        if (cancelled) return undefined;
        const sizes = pages.map((page) => {
          const viewport = page.getViewport({ scale: 1 });
          return { width: viewport.width, height: viewport.height };
        });
        setPdf({ pages, sizes });
      } catch {
        if (!cancelled) setRenderFailed(true);
      }
      return undefined;
    };
    void load();
    return () => {
      cancelled = true;
      destroy?.();
      setPdf(null);
    };
  }, [state]);

  const layoutAt = useMemo<LayoutAt>(
    () => (pdf ? (zoom, viewport) => columnLayout(pdf.sizes.map((size) => scaled(size, pdfFit(size, viewport.width) * zoom)), viewport, "top") : null),
    [pdf],
  );
  const { zoom, layout, scroller, scrollerRef } = useViewerZoom(displayId, layoutAt);
  const [drawer, setDrawer] = useState<PdfPages | null>(null);
  useEffect(() => {
    if (!pdf || !scroller) return undefined;
    const pages = new PdfPages(pdf.pages, pdf.sizes, scroller, () => setRenderFailed(true), useShellStore.getState().noteDiagnostic);
    setDrawer(pages);
    return () => {
      pages.close();
      setDrawer(null);
    };
  }, [pdf, scroller]);
  useEffect(() => {
    if (drawer && layout) drawer.show(layout);
  }, [drawer, layout]);

  if (state.status === "failed") return <ViewerNotice state="pdf-failed" reason={t("documents.pdfUnavailable", { reason: state.reason })} />;
  if (renderFailed) return <ViewerNotice state="pdf-render" reason={t("documents.pdfRenderFailed")} />;
  if (state.status !== "ready") return <ViewerNotice state="pdf-loading" reason={t("documents.pdfLoading")} />;
  return (
    <div ref={scrollerRef} className="relative min-h-0 flex-1 overflow-auto [overflow-anchor:none] [scrollbar-gutter:stable]" data-viewer="pdf" data-viewer-zoom={zoom}>
      {layout ? (
        <div className="relative" style={{ width: layout.width, height: layout.height }}>
          {layout.boxes.map((box, index) => (
            <div key={index} className="absolute" style={{ left: box.left, top: box.top, width: box.width, height: box.height }} data-pdf-page={index + 1} />
          ))}
        </div>
      ) : null}
    </div>
  );
}
