import { StrictMode } from "react";
import type { Root } from "react-dom/client";
import { Gallery, GalleryFrame } from "./Gallery";
import { GALLERY, type Section } from "./manifest";

export function mountGallery(root: Root) {
  const params = new URLSearchParams(window.location.search);
  const frame = params.get("frame");
  const theme = params.get("theme") === "light" ? "light" : "dark";
  if (frame) {
    // A frame's layer takes focus as it opens; without this the gallery page
    // around the frame would scroll to it on every load.
    const focus = HTMLElement.prototype.focus;
    HTMLElement.prototype.focus = function (options?: FocusOptions) {
      focus.call(this, { ...options, preventScroll: true });
    };
    const cut = frame.indexOf("/");
    const section = frame.slice(0, cut) as Section;
    if (!(section in GALLERY)) throw new Error(`Unknown gallery section ${section}`);
    root.render(
      <StrictMode>
        <GalleryFrame section={section} state={frame.slice(cut + 1)} theme={theme} />
      </StrictMode>,
    );
    return;
  }
  document.title = "hide · System gallery";
  root.render(
    <StrictMode>
      <Gallery />
    </StrictMode>,
  );
}
