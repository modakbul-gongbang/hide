#!/usr/bin/env node
// Registers an isolated device with the measured hided and brings its
// fixture workspace to the front, the way the shell does (`register_device`
// with consent, then `remote_control` `focus_workspace` with
// `focus_device`). usage: device-front.mjs register|front
// Reads MEASURE_HIDED_PORT, MEASURE_HIDED_TOKEN, MEASURE_DEVICE_ID,
// MEASURE_DEVICE_ALIAS and MEASURE_DEVICE_SOCKET; `front` returns once the
// device is connected, in front and its helper ready, within
// MEASURE_DEVICE_READY_SECONDS (900: the first install uploads the build).
const port = process.env.MEASURE_HIDED_PORT;
const token = process.env.MEASURE_HIDED_TOKEN;
const device = process.env.MEASURE_DEVICE_ID;
if (!port || !token || !device) throw new Error("MEASURE_HIDED_PORT, MEASURE_HIDED_TOKEN and MEASURE_DEVICE_ID are required");

/** One socket: the handshake, then every frame through `onFrame` until it answers true. */
function session(onFrame, deadlineMs) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, { headers: { origin: `http://127.0.0.1:${port}` } });
    const timer = setTimeout(() => { ws.close(); reject(new Error("timed out")); }, deadlineMs);
    let rest = null;
    ws.onerror = (error) => { clearTimeout(timer); reject(error); };
    ws.onopen = () => ws.send(JSON.stringify({ token, schema_version: 2 }));
    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      const frame = JSON.parse(event.data);
      if (frame.type === "snapshot") rest = frame.payload?.rest ?? rest;
      else if (frame.type === "delta" && frame.payload?.rest) rest = { ...rest, ...frame.payload.rest };
      const send = (body) => ws.send(JSON.stringify({ schema_version: 2, ...body }));
      const done = onFrame(frame, rest, send);
      if (done !== undefined && done !== false) {
        clearTimeout(timer);
        ws.close();
        resolve(done);
      }
    };
  });
}

const mode = process.argv[2];
if (mode === "register") {
  await session((frame, rest, send) => {
    if (frame.type !== "snapshot") return false;
    send({ kind: "register_device", payload: {
      id: device, label: device, ssh_alias: process.env.MEASURE_DEVICE_ALIAS,
      herdr_socket_path: process.env.MEASURE_DEVICE_SOCKET, host_consent: true,
    } });
    return true;
  }, 10_000);
  console.log("registered");
} else if (mode === "front") {
  let asked = false;
  await session((_frame, rest, send) => {
    const remote = (rest?.status?.remote ?? []).find((row) => row.target_id === device);
    const workspace = remote?.session?.workspaces?.[0];
    const checkout = workspace?.checkouts?.[0];
    if (remote?.state !== "connected" || !workspace || !checkout) return false;
    if (!asked) {
      asked = true;
      send({ kind: "remote_control", payload: {
        target_id: device, request_id: `measure-${Date.now()}`, action: "focus_workspace",
        workspace_id: workspace.id, checkout_id: checkout.id, focus_device: true,
      } });
    }
    // In front, and the device's node (its helper) installed and running:
    // its terminals ride that link.
    const host = (rest?.navigator?.devices ?? []).find((row) => row.id === device)?.host;
    return rest?.navigator?.focused_device_id === device && host?.state === "ready";
  }, Number(process.env.MEASURE_DEVICE_READY_SECONDS ?? 900) * 1000);
  console.log("front");
} else {
  throw new Error("usage: device-front.mjs register|front");
}
