// GPL-3.0-only. Appended to hid_tap.js with per-run parameters by the helper.
// Outbound authenticated loopback only; no reconnect and no listening socket.
const diagnostic = rpc.exports;
let connection = null;
let pendingWrites = 0;
let writes = Promise.resolve();
let finishing = false;
let handshakeTimer = null;

function ascii(text) {
  return Array.from(text, value => value.charCodeAt(0));
}

async function finishTransport(stopHook) {
  if (finishing) return;
  finishing = true;
  if (stopHook) diagnostic.stop();
  if (handshakeTimer !== null) clearTimeout(handshakeTimer);
  handshakeTimer = null;
  try { await writes; } catch (_error) {}
  if (connection !== null) {
    try { await connection.close(); } catch (_error) {}
  }
}

reportSink = payload => {
  if (connection === null) return;
  if (pendingWrites >= 128) { void finishTransport(true); return; }
  pendingWrites++;
  writes = writes.then(() => connection.output.writeAll(ascii(JSON.stringify(payload) + "\n")))
    .catch(() => { void finishTransport(true); })
    .finally(() => { pendingWrites--; });
  if (payload.kind === "stopped") void finishTransport(false);
};

async function connectOnce() {
  handshakeTimer = setTimeout(() => { void finishTransport(true); }, 10000);
  try {
    const connected = await Socket.connect({family: "ipv4", host: "127.0.0.1", port: RUN_PARAMETERS.port});
    if (finishing) { await connected.close(); return; }
    connection = connected;
    await connection.output.writeAll(ascii(JSON.stringify({
      kind: "hello", token: RUN_PARAMETERS.token, pid: Process.id
    }) + "\n"));
    let buffered = "";
    let authenticated = false;
    while (!finishing) {
      const bytes = new Uint8Array(await connection.input.read(2048));
      if (bytes.length === 0) break;
      buffered += String.fromCharCode(...bytes);
      if (buffered.length > 4096) throw new Error("control_message_too_large");
      let newline;
      while ((newline = buffered.indexOf("\n")) >= 0) {
        const command = JSON.parse(buffered.slice(0, newline));
        buffered = buffered.slice(newline + 1);
        if (!authenticated && command.kind === "accepted") {
          authenticated = true;
          clearTimeout(handshakeTimer);
          handshakeTimer = null;
          diagnostic.start(RUN_PARAMETERS.capture);
        } else if (authenticated && command.kind === "stop") {
          await finishTransport(true);
          return;
        } else if (authenticated && command.kind === "lease" && RUN_PARAMETERS.capture.renewable === true) {
          diagnostic.renewLease();
        } else {
          throw new Error("unexpected_control_message");
        }
      }
    }
  } catch (_error) {}
  await finishTransport(true);
}

rpc.exports = {
  init() { void connectOnce(); },
  dispose() { return finishTransport(true); }
};
