import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./index.css";

const container = document.getElementById("root");
if (!container) throw new Error("root missing");
const root = createRoot(container);

// The System gallery exists only in a dev build: `import.meta.env.DEV` is
// false in `vite build`, so the import is dropped and hided never serves it.
if (import.meta.env.DEV && window.location.pathname === "/gallery") {
  void import("./gallery/mount").then(({ mountGallery }) => mountGallery(root));
} else {
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
