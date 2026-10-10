#!/usr/bin/env node
// Brings a checkout of a remote-core measurement to the front through the
// screen machine's node-role hided, the way the shell does. usage:
//   core-front.mjs screen   the screen machine's fixture checkout: opened on
//                           the core as that node's workspace (`create_workspace`
//                           for the node), then `remote_control` `focus_workspace`
//                           with `focus_device`
//   core-front.mjs core     the core machine's own fixture checkout at
//                           MEASURE_CORE_FIXTURE, opened on the core when it
//                           is not yet (`create_workspace`), then
//                           `focus_checkout` with `focus_device`
// Reads MEASURE_HIDED_PORT, MEASURE_HIDED_TOKEN, MEASURE_SCREEN_NODE,
// MEASURE_CORE_NODE and MEASURE_FIXTURE; returns once the checkout is in
// front, within MEASURE_FRONT_SECONDS (120). Prints the front pane's id.
const port = process.env.MEASURE_HIDED_PORT;
const token = process.env.MEASURE_HIDED_TOKEN;
const screenNode = process.env.MEASURE_SCREEN_NODE;
const coreNode = process.env.MEASURE_CORE_NODE;
const fixture = process.env.MEASURE_FIXTURE;
if (!port || !token || !screenNode || !coreNode || !fixture) {
  throw new Error("MEASURE_HIDED_PORT, MEASURE_HIDED_TOKEN, MEASURE_SCREEN_NODE, MEASURE_CORE_NODE and MEASURE_FIXTURE are required");
}

/** One socket: the handshake, then every frame through `onFrame` until it answers a value. */
function session(onFrame, deadlineMs) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, { headers: { origin: `http://127.0.0.1:${port}` } });
    let rest = null;
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error(`timed out; last state: ${JSON.stringify({ devices: rest?.navigator?.devices, remote: rest?.status?.remote?.map((row) => ({ id: row.target_id, state: row.state, paths: row.session?.workspaces?.map((w) => w.path) })), front: rest?.navigator?.focused_device_id })}`));
    }, deadlineMs);
    ws.onerror = (error) => { clearTimeout(timer); reject(error); };
    ws.onopen = () => ws.send(JSON.stringify({ token, schema_version: 2 }));
    ws.onmessage = (event) => {
      if (typeof event.data !== "string") return;
      const frame = JSON.parse(event.data);
      if (frame.type === "snapshot") rest = frame.payload?.rest ?? rest;
      else if (frame.type === "delta" && frame.payload?.rest) rest = { ...rest, ...frame.payload.rest };
      if (!rest) return;
      const send = (body) => ws.send(JSON.stringify({ schema_version: 2, ...body }));
      const done = onFrame(rest, send);
      if (done !== undefined && done !== false) {
        clearTimeout(timer);
        ws.close();
        resolve(done);
      }
    };
  });
}

const deadline = Number(process.env.MEASURE_FRONT_SECONDS ?? 120) * 1000;
const mode = process.argv[2];
let asked = false;
let focused = false;
if (mode === "screen") {
  const pane = await session((rest, send) => {
    const remote = (rest.status?.remote ?? []).find((row) => row.target_id === screenNode);
    if (remote?.state !== "connected") return false;
    // A checkout of a repository is grouped under its repository's project.
    const workspace = (remote.session?.workspaces ?? []).find((row) => row.path === fixture || (row.checkouts ?? []).some((checkout) => checkout.path === fixture));
    if (!workspace) {
      if (!asked) {
        asked = true;
        send({ kind: "create_workspace", payload: { device_id: screenNode, path: fixture, label: "measure", initialize_git: false } });
      }
      return false;
    }
    const checkout = workspace.checkouts?.find((row) => row.path === fixture) ?? workspace.checkouts?.[0];
    if (!checkout) return false;
    if (!focused) {
      focused = true;
      send({ kind: "remote_control", payload: {
        target_id: screenNode, request_id: `measure-${Date.now()}`, action: "focus_workspace",
        workspace_id: workspace.id, checkout_id: checkout.id, focus_device: true,
      } });
    }
    if (rest.navigator?.focused_device_id !== screenNode) return false;
    return remote.session?.focused_pane_id ?? false;
  }, deadline);
  console.log(pane);
} else if (mode === "core") {
  const pane = await session((rest, send) => {
    const path = process.env.MEASURE_CORE_FIXTURE;
    if (!path) throw new Error("core-front.mjs core needs MEASURE_CORE_FIXTURE");
    const workspace = (rest.navigator?.workspaces ?? []).find((row) => (row.checkouts ?? []).some((checkout) => checkout.path === path));
    const checkout = workspace?.checkouts?.find((row) => row.path === path);
    if (!workspace || !checkout) {
      if (!asked) {
        asked = true;
        send({ kind: "create_workspace", payload: { device_id: coreNode, path, label: "measure-core", initialize_git: false } });
      }
      return false;
    }
    if (!focused) {
      focused = true;
      send({ kind: "focus_checkout", payload: { workspace_id: workspace.id, checkout_id: checkout.id, focus_device: true } });
    }
    if ((rest.navigator?.focused_device_id ?? coreNode) !== coreNode) return false;
    return rest.focused?.pane_id ?? false;
  }, deadline);
  console.log(pane);
} else {
  throw new Error("usage: core-front.mjs screen|core");
}
