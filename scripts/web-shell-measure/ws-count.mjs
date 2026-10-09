#!/usr/bin/env node
// Counts the page's /ws frames for MEASURE_WS_COUNT_SECONDS: those it
// received by `type`, those it sent by `kind`, and the full redraws it asked
// for (`terminal_viewport` with `new_view`), the one request every build's
// shell sends for a whole frame of a pane. A build whose daemon asks the
// pane for a redraw on its own logs that in its diagnostics instead
// (run.sh reads both). Connect before the page navigates away: the counts
// follow the tab across navigations.
import { connectPage } from "./cdp.mjs";

const cdpPort = process.env.MEASURE_CDP_PORT;
const seconds = Number(process.env.MEASURE_WS_COUNT_SECONDS ?? 0);
if (!cdpPort || !(seconds > 0)) throw new Error("MEASURE_CDP_PORT and MEASURE_WS_COUNT_SECONDS are required");

const page = await connectPage(cdpPort);
const received = {};
const sent = {};
let fullRedrawsAsked = 0;
const tally = (into, key, data) => {
  const name = key.exec(data ?? "")?.[1] ?? "binary";
  into[name] = (into[name] ?? 0) + 1;
};
page.on("Network.webSocketFrameReceived", ({ response }) => tally(received, /"type":"([a-z_]+)"/, response?.payloadData));
page.on("Network.webSocketFrameSent", ({ response }) => {
  const data = response?.payloadData ?? "";
  tally(sent, /"kind":"([a-z_]+)"/, data);
  if (data.includes('"kind":"terminal_viewport"') && data.includes('"new_view":true')) fullRedrawsAsked += 1;
});
await page.send("Network.enable");
await new Promise((resolve) => setTimeout(resolve, seconds * 1000));
page.close();
console.log(JSON.stringify({ seconds, received_by_type: received, sent_by_kind: sent, full_redraws_asked: fullRedrawsAsked }));
