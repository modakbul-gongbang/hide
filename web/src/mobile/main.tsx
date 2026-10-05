// The phone app's entry (PRD mobile-companion D-02, D-16, B38): follow the
// phone's light or dark setting, register the service worker that caches the
// static shell and shows pushes, and start the one socket.

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "../index.css";
import "./mobile.css";
import { App } from "./App";
import { openDetail, start } from "./connection";
import { PhoneLanguageBoundary } from "./language";
import { openKey } from "./protocol";

const scheme = window.matchMedia("(prefers-color-scheme: dark)");
const followScheme = () => document.documentElement.classList.toggle("dark", scheme.matches);
followScheme();
scheme.addEventListener("change", followScheme);

if ("serviceWorker" in navigator) {
  void navigator.serviceWorker.register("/m/sw.js", { scope: "/m/" }).catch(() => undefined);
  // A tapped notification while the app is open: the worker names the agent (B32).
  navigator.serviceWorker.addEventListener("message", (event: MessageEvent<{ type?: string; tag?: string }>) => {
    const key = event.data?.type === "open" && event.data.tag ? openKey(event.data.tag) : null;
    if (key) openDetail(key);
  });
}

start();

const container = document.getElementById("root");
if (!container) throw new Error("root missing");
createRoot(container).render(
  <StrictMode>
    <PhoneLanguageBoundary />
    <App />
  </StrictMode>,
);
