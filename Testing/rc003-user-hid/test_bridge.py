"""Offline bridge tests. No elevation, process attachment, or key injection."""
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import capture
import sayall_bridge as bridge


class BridgeTests(unittest.TestCase):
    def test_only_three_buttons_valid_scopes_and_bounded_streams(self):
        valid = {"kind": "state", "stream": 1, "scope": "proxy_unverified", "active": ["back"]}
        self.assertEqual(bridge.validate_state(valid)["active"], ["back"])
        for changed in ({"stream": True}, {"stream": 17}, {"scope": "keyboard"},
                        {"active": ["ok"]}, {"active": ["back", "back"]}, {"active": "back"}):
            with self.assertRaises(capture.ProbeError):
                bridge.validate_state({**valid, **changed})

    def test_peer_must_match_the_selected_address_not_just_the_model(self):
        target = capture.Target(1, "fixture", capture.HARDWARE + "_001122334455", "fixture", 1)
        self.assertTrue(bridge.peer_matches(target, "001122334455"))
        self.assertFalse(bridge.peer_matches(target, "001122334456"))
        self.assertFalse(bridge.peer_matches(target, ""))
        target = capture.Target(1, "fixture", capture.HARDWARE, "fixture", 1)
        self.assertFalse(bridge.peer_matches(target, "001122334455"))

    def test_control_reader_keeps_partial_frames_and_bounds_input(self):
        class Socket:
            data = [b'{"kind":"lea', b'se"}\n{"kind":"stop"}\n', b'x' * 4097]
            def recv(self, _size): return self.data.pop(0)
        channel = bridge.Channel.__new__(bridge.Channel)
        channel.socket, channel.buffer = Socket(), b""
        self.assertEqual(channel.commands(), [])
        self.assertEqual(channel.commands(), [{"kind": "lease"}, {"kind": "stop"}])
        with self.assertRaises(capture.ProbeError): channel.commands()

    def run_fixture(self, initial_command="lease", change_stream=False, shared=1):
        class Channel:
            peer = "001122334455"
            def __init__(self): self.events, self.calls = [], 0
            def send(self, kind, **fields): self.events.append({"kind": kind, **fields})
            def commands(self):
                self.calls += 1
                if self.calls == 1: return [{"kind": initial_command}]
                if self.calls == 2: return []
                return [{"kind": "stop"}]
        class Handle:
            def __init__(self, *_args): pass
            def close(self): return True
            def alive(self): return True
            def verify_signature(self): pass
            def inspect(self): return {"dynamic_code_flags": 0, "signature_flags": 0, "protection_level": 0xFFFFFFFE}
        class Flag:
            def is_set(self): return False
        class Backend:
            starts = 0
            def __init__(self, _base, _target, _host, _log, callback, seconds, renewable):
                self.callback, self.failed = callback, Flag()
                assert seconds == 15 and renewable is True
            def start(self):
                Backend.starts += 1
                for stream, active in [(1, ["back"]), (2 if change_stream else 1, [])]:
                    self.callback({"type": "send", "payload": {"kind": "state", "scope": "proxy_unverified", "stream": stream, "active": active}}, None)
            def stop(self): return True
            def renew_lease(self): pass
        channel = Channel()
        target = capture.Target(1, "fixture", capture.HARDWARE + "_001122334455", "fixture", shared)
        with patch.multiple(capture, is_admin=lambda: True,
                security_state=lambda: {"experimental_filter_present": False}, discover=lambda: target,
                CaptureLock=Handle, DebugPrivilege=Handle, HostHandle=Handle), patch.object(bridge.gadget_backend, "Backend", Backend):
            bridge.serve(channel)
        return channel.events, Backend.starts

    def test_stop_before_attachment_never_starts_gadget(self):
        events, starts = self.run_fixture("stop")
        self.assertEqual(starts, 0)
        self.assertEqual(events[-1], {"kind": "stopped", "reason": "stop_requested", "cleanup": True})

    def test_shared_host_is_rejected_before_attachment(self):
        events, starts = self.run_fixture(shared=2)
        self.assertEqual(starts, 0)
        self.assertEqual(events[-1]["reason"], "selected_remote_or_host_mismatch")

    def test_bridge_forwards_only_valid_states_and_stops_cleanly(self):
        events, starts = self.run_fixture()
        self.assertEqual(starts, 1)
        self.assertEqual([event["active"] for event in events if event["kind"] == "state"], [["back"], []])
        self.assertEqual(events[-1]["reason"], "stop_requested")

    def test_stream_change_and_unknown_control_stop_capture(self):
        events, _ = self.run_fixture(change_stream=True)
        self.assertEqual(events[-1]["reason"], "capture_stream_changed")
        events, starts = self.run_fixture("execute")
        self.assertEqual(starts, 0)
        self.assertEqual(events[-1]["reason"], "app_command_invalid")


if __name__ == "__main__":
    unittest.main()
