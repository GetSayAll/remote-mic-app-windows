// SPDX-License-Identifier: GPL-3.0-only
// Protocol and handle-scoping reference: Axonkey a0451ec, see README.md.
// Diagnostic only: never change arguments, reports, return values, or keys.
"use strict";

const READ_CHARACTERISTIC = 0x80018483;
const PROXY_OBJECT = /^\\device\\umdfctrldev-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const BUTTONS = new Map([[0x80, "volume_up"], [0x81, "volume_down"], [0xf1, "back"]]);
let reportSink = payload => send(payload);
let listener = null;
let timer = null;
let heartbeat = null;
let expired = false;
let started = false;
let allow = "";
let allowProxy = false;
let renewable = false;

function renewLease() {
  if (!renewable || listener === null || expired) throw new Error("lease_not_active");
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => { expired = true; stop("lease_expired"); }, 15000);
}
const streams = new Map();
const completions = new Set();
const counts = {
  total_ioctl: 0, ioctl: 0, direct: 0, proxy: 0, unrelated: 0, pending: 0,
  errors: 0, wrong_length: 0, unspecified_length: 0, invalid_report: 0, reports: 0, stream_limit: 0
};

function scopeOf(name) {
  if (name === allow) return "rc003";
  if (allowProxy && PROXY_OBJECT.test(name)) return "proxy_unverified";
  return "unrelated";
}

function parseReport(bytes) {
  if (bytes.length !== 9 || bytes[0] !== 1 || bytes[1] !== 0 || bytes[2] !== 0) return null;
  const selected = new Set();
  for (let offset = 3; offset < 9; offset += 2) {
    const button = BUTTONS.get(bytes[offset] | (bytes[offset + 1] << 8));
    if (button) selected.add(button);
  }
  return [...selected].sort();
}

function stats() {
  return { kind: "stats", hook_installed: listener !== null, streams: streams.size, ...counts };
}

function stop(reason) {
  if (listener !== null) listener.detach();
  listener = null;
  if (timer !== null) clearTimeout(timer);
  if (heartbeat !== null) clearInterval(heartbeat);
  timer = null;
  heartbeat = null;
  reportSink({ ...stats(), kind: "stopped", reason });
  return stats();
}

rpc.exports = {
  start(config) {
    if (started || expired) throw new Error("capture_already_started_or_expired");
    if (Process.platform !== "windows" || Process.arch !== "x64") throw new Error("unsupported_host");
    if (!config || typeof config.device !== "string" || !/^\\device\\[^\\]+$/i.test(config.device)) {
      throw new Error("invalid_allowlist");
    }
    if (!Number.isInteger(config.seconds) || config.seconds < 15 || config.seconds > 180) {
      throw new Error("invalid_duration");
    }
    // Do not stack this experiment with an existing Gadget tap.
    if (Process.enumerateModules().some(m => /axonkeyrc003hidtap|frida[-_]gadget/i.test(m.name))) {
      throw new Error("existing_gadget_detected");
    }
    allow = config.device.toLowerCase();
    allowProxy = config.proxy_diagnostics === true;
    renewable = config.renewable === true;
    started = true;
    const ntdll = Process.getModuleByName("ntdll.dll");
    const query = new NativeFunction(ntdll.getExportByName("NtQueryObject"), "int",
      ["pointer", "uint", "pointer", "uint", "pointer"]);
    function nameOf(handle) {
      // Query each call: Windows can reuse a closed handle for another device.
      const buffer = Memory.alloc(4096);
      const required = Memory.alloc(4);
      if (query(handle, 1, buffer, 4096, required) !== 0) return "";
      const length = buffer.readU16();
      const address = buffer.add(Process.pointerSize).readPointer();
      if (address.isNull() || length < 2 || length > 4000 || length % 2 !== 0) return "";
      return address.readUtf16String(length / 2).toLowerCase();
    }
    listener = Interceptor.attach(ntdll.getExportByName("NtDeviceIoControlFile"), {
      onEnter(args) {
        this.capture = false;
        counts.total_ioctl++;
        if (args[5].toUInt32() !== READ_CHARACTERISTIC) return;
        counts.ioctl++;
        try {
          const name = nameOf(args[0]);
          const scope = scopeOf(name);
          if (scope === "unrelated") { counts.unrelated++; return; }
          counts[scope === "rc003" ? "direct" : "proxy"]++;
          if (args[9].toUInt32() !== 9) { counts.wrong_length++; return; }
          const identity = args[0].toString() + ":" + name;
          if (!streams.has(identity)) {
            if (streams.size >= 16) { counts.stream_limit++; return; }
            const stream = { id: streams.size + 1, scope, previous: null };
            streams.set(identity, stream);
            reportSink({ kind: "stream", stream: stream.id, scope });
          }
          this.stream = streams.get(identity);
          this.buffer = args[8];
          this.status = args[4];
          this.capture = true;
        } catch (_error) { counts.errors++; }
      },
      onLeave(result) {
        if (!this.capture || listener === null) return;
        const status = result.toUInt32();
        // Pending buffers belong to the OS. Never read or retain them for later.
        if (status === 0x103) { counts.pending++; return; }
        if (status !== 0) { counts.errors++; return; }
        try {
          if (this.buffer.isNull() || this.status.isNull()) { counts.errors++; return; }
          const completionStatus = this.status.readU32();
          const informationText = this.status.add(Process.pointerSize).readU64().toString(10);
          const information = /^\d{1,4}$/.test(informationText) && Number(informationText) <= 4096
            ? Number(informationText) : -1;
          const completionKey = completionStatus + ":" + information;
          if (!completions.has(completionKey) && completions.size < 4) {
            completions.add(completionKey);
            reportSink({kind: "completion", scope: this.stream.scope, io_status: completionStatus,
              information, declared_length: 9, returned_status: 0});
          }
          // This host's METHOD_NEITHER proxy returns success with Information=0.
          // The declared output capacity is still exactly 9. Never accept partial
          // lengths or pending I/O; validate the protocol header before reporting.
          if (completionStatus !== 0 || (information !== 0 && information !== 9)) {
            counts.wrong_length++; return;
          }
          if (information === 0) counts.unspecified_length++;
          const active = parseReport(new Uint8Array(this.buffer.readByteArray(9)));
          if (active === null) { counts.invalid_report++; return; }
          counts.reports++;
          const serialized = JSON.stringify(active);
          if (this.stream.previous === serialized) return;
          this.stream.previous = serialized;
          // No raw buffers, device names, other keys, or voice payloads leave the host.
          reportSink({ kind: "state", stream: this.stream.id, scope: this.stream.scope, active });
        } catch (_error) { counts.errors++; }
      }
    });
    if (renewable) renewLease();
    else timer = setTimeout(() => { expired = true; stop("host_deadline"); }, config.seconds * 1000);
    heartbeat = setInterval(() => reportSink(stats()), 5000);
    reportSink({ kind: "ready", hook_installed: true, proxy_diagnostics: allowProxy });
    return { hook_installed: true };
  },
  stop() { return stop("requested"); },
  renewLease
};
