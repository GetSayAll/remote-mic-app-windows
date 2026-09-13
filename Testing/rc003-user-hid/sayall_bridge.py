"""GPL-3.0-only. Optional elevated RC003 capture bridge; never executes actions.

Only an explicitly launched SayAll instance can renew the capture lease.
No installation, autostart, arbitrary commands, keyboard injection, or reconnect.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import queue
import re
import socket
import time

import capture
import gadget_backend


class Channel:
    def __init__(self, port, token):
        self.socket = socket.create_connection(("127.0.0.1", port), timeout=3)
        self.socket.settimeout(0.1)
        self.buffer = b""
        self.sequence = 0
        self.send("hello", token=token, protocol=1)
        deadline = time.monotonic() + 5
        accepted = None
        while accepted is None and time.monotonic() < deadline:
            for command in self.commands():
                if accepted is not None or command.get("kind") != "accepted":
                    raise capture.ProbeError("app_handshake_invalid")
                accepted = command
        peer = accepted.get("peer") if accepted else None
        if not isinstance(peer, str) or not re.fullmatch(r"[0-9a-f]{12}", peer):
            raise capture.ProbeError("app_peer_missing")
        self.peer = peer

    def send(self, kind, **fields):
        message = {"kind": kind, "seq": self.sequence, **fields}
        self.sequence += 1
        self.socket.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("ascii"))

    def commands(self):
        try:
            data = self.socket.recv(2048)
        except socket.timeout:
            return []
        if not data:
            raise capture.ProbeError("app_disconnected")
        self.buffer += data
        if len(self.buffer) > 4096:
            raise capture.ProbeError("app_message_too_large")
        commands = []
        while b"\n" in self.buffer:
            line, self.buffer = self.buffer.split(b"\n", 1)
            command = json.loads(line)
            if not isinstance(command, dict):
                raise capture.ProbeError("app_message_invalid")
            commands.append(command)
        return commands

    def close(self):
        self.socket.close()


class BridgeLog:
    def __init__(self, channel):
        self.channel = channel
        self.cleanup = None

    def emit(self, event, **fields):
        if event == "cleanup" and fields.get("phase") == "gadget_hook":
            self.cleanup = fields.get("result") == "passed"
        # Only fixed lifecycle categories cross this second IPC boundary.
        self.channel.send("diagnostic", event=event,
                          result=fields.get("result", "observed"))


def validate_state(payload):
    if not isinstance(payload, dict) or payload.get("kind") != "state":
        raise capture.ProbeError("invalid_state")
    if type(payload.get("stream")) is not int or not 1 <= payload["stream"] <= 16:
        raise capture.ProbeError("invalid_stream")
    if payload.get("scope") not in capture.SCOPES:
        raise capture.ProbeError("invalid_scope")
    active = payload.get("active")
    if (not isinstance(active, list) or len(active) > 3 or
            any(not isinstance(key, str) or key not in capture.BUTTONS for key in active) or
            len(set(active)) != len(active)):
        raise capture.ProbeError("invalid_buttons")
    return {"stream": payload["stream"], "scope": payload["scope"], "active": active}


def peer_matches(target, peer):
    return (bool(re.fullmatch(r"[0-9a-f]{12}", peer)) and
            target.service.lower() == capture.HARDWARE + "_" + peer)


def serve(channel):
    log = BridgeLog(channel)
    backend = host = privilege = lock = None
    reason = "stop_requested"
    cleanup = True
    inbox = queue.Queue(maxsize=128)
    overflow = False

    def on_message(message, _data):
        nonlocal overflow
        try:
            inbox.put_nowait(message)
        except queue.Full:
            overflow = True

    try:
        if not capture.is_admin():
            raise capture.ProbeError("administrator_launch_required")
        if capture.security_state()["experimental_filter_present"]:
            raise capture.ProbeError("kernel_filter_conflict")
        target = capture.discover()
        if target.shared_hid_count != 1 or not peer_matches(target, channel.peer):
            raise capture.ProbeError("selected_remote_or_host_mismatch")
        lock = capture.CaptureLock()
        privilege = capture.DebugPrivilege()
        host = capture.HostHandle(target.pid)
        host.verify_signature()
        policy = host.inspect()
        if (policy["dynamic_code_flags"] is None or policy["signature_flags"] is None or
                policy["dynamic_code_flags"] & 1 or policy["signature_flags"] & 3 or
                policy["protection_level"] != 0xFFFFFFFE):
            raise capture.ProbeError("host_policy_incompatible")
        # Check that the app is still present before any host injection.
        for command in channel.commands():
            if command.get("kind") == "stop":
                return
            if command.get("kind") != "lease":
                raise capture.ProbeError("app_command_invalid")
        backend = gadget_backend.Backend(Path(__file__).resolve().parent, target, host,
                                        log, on_message, 15, renewable=True)
        backend.start()
        channel.send("ready", scope="proxy_unverified")
        last_app = last_host = last_health = last_lease = time.monotonic()
        source = None
        while True:
            for command in channel.commands():
                if command.get("kind") == "stop":
                    return
                if command.get("kind") != "lease":
                    raise capture.ProbeError("app_command_invalid")
                last_app = time.monotonic()
            now = time.monotonic()
            if now - last_app > 5:
                raise capture.ProbeError("app_lease_expired")
            if now - last_host > 12 or backend.failed.is_set() or overflow:
                raise capture.ProbeError("capture_health_lost")
            if now - last_health >= 2:
                current = capture.discover()
                if current != target or not host.alive():
                    raise capture.ProbeError("hid_host_changed")
                last_health = now
            if now - last_lease >= 1:
                backend.renew_lease()
                channel.send("heartbeat")
                last_lease = now
            for _ in range(128):
                try:
                    message = inbox.get_nowait()
                except queue.Empty:
                    break
                if message.get("type") != "send" or not isinstance(message.get("payload"), dict):
                    raise capture.ProbeError("agent_message_invalid")
                payload = message["payload"]
                kind = payload.get("kind")
                if kind == "state":
                    state = validate_state(payload)
                    identity = (state["stream"], state["scope"])
                    if source is not None and source != identity:
                        raise capture.ProbeError("capture_stream_changed")
                    source = identity
                    channel.send("state", **state)
                elif kind in {"stats", "ready"}:
                    if kind == "stats" and (payload.get("errors", 0) or payload.get("stream_limit", 0)):
                        raise capture.ProbeError("capture_hook_error")
                    last_host = time.monotonic()
                elif kind == "stopped":
                    raise capture.ProbeError("capture_hook_stopped")
    except Exception as error:
        candidate = getattr(error, "reason", "bridge_exception")
        reason = candidate if re.fullmatch(r"[a-z0-9_]{1,100}", str(candidate)) else "bridge_exception"
    finally:
        if backend is not None:
            try:
                cleanup = backend.stop()
            except Exception:
                # The app can close before the cleanup diagnostic is written.
                cleanup = backend.detached.is_set()
        if host is not None:
            host.close()
        if privilege is not None:
            cleanup = privilege.close() and cleanup
        if lock is not None:
            lock.close()
        try:
            channel.send("stopped", reason=reason, cleanup=cleanup)
        except OSError:
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--token", required=True)
    args = parser.parse_args()
    if not 1024 <= args.port <= 65535 or not re.fullmatch(r"[0-9a-f]{64}", args.token):
        return 2
    channel = None
    try:
        channel = Channel(args.port, args.token)
        serve(channel)
        return 0
    except Exception:
        return 1
    finally:
        if channel is not None:
            channel.close()


if __name__ == "__main__":
    raise SystemExit(main())
