"""Offline tests: importing this module never imports Frida or opens a process."""
import importlib.util
import ctypes
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("rc003_capture", Path(__file__).with_name("capture.py"))
capture = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = capture
SPEC.loader.exec_module(capture)
GADGET_SPEC = importlib.util.spec_from_file_location("gadget_backend", Path(__file__).with_name("gadget_backend.py"))
gadget = importlib.util.module_from_spec(GADGET_SPEC)
GADGET_SPEC.loader.exec_module(gadget)


class LedgerTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        self.ledger = capture.Ledger(lambda event, **fields: self.events.append((event, fields)))

    def state(self, active, stream=1, scope="rc003"):
        self.ledger.state({"stream": stream, "scope": scope, "active": active})

    def test_three_keys_repeats_and_complete_cycles(self):
        for button in sorted(capture.BUTTONS):
            for _ in range(3):
                self.state([button])
                self.state([button])
                self.state([])
                self.state([])
        self.assertEqual(len(self.events), 18)
        self.assertEqual(self.ledger.finish()["complete_cycles"]["rc003"],
                         {"back": 3, "volume_down": 3, "volume_up": 3})

    def test_combinations_order_and_switch(self):
        self.state(["back", "volume_up"])
        self.state(["volume_up", "back"])
        self.assertEqual(len(self.events), 2)
        self.state(["volume_down"])
        self.assertEqual([event[1]["edge"] for event in self.events], ["down", "down", "up", "up", "down"])

    def test_invalid_messages_never_release_a_key(self):
        self.state(["back"])
        for active in (None, "back", ["a"], ["back", "back"], [1], [["back"]]):
            self.state(active)
        for scope in (None, [], {}, "other"):
            self.state([], scope=scope)
        for stream in (True, "1", 0, 17, None):
            self.state([], stream=stream)
        self.assertEqual(len(self.events), 1)
        self.assertEqual(self.ledger.rejected, 15)
        self.state([])
        self.assertEqual(self.ledger.cycles["rc003"]["back"], 1)

    def test_proxy_never_confirms_device(self):
        self.state(["back"], stream=2, scope="proxy_unverified")
        self.state([], stream=2, scope="proxy_unverified")
        result = self.ledger.finish()
        self.assertEqual(result["complete_cycles"]["proxy_unverified"]["back"], 1)
        self.assertEqual(result["complete_cycles"]["rc003"]["back"], 0)
        self.assertFalse(result["physical_keys_verified"])
        self.assertFalse(result["sayall_integrated"])

    def test_streams_and_scope_changes_are_isolated(self):
        self.state(["back"], stream=1)
        self.state(["back"], stream=2)
        self.state([], stream=1, scope="proxy_unverified")
        self.state([], stream=3)
        self.assertEqual(len(self.events), 2)
        self.state([], stream=1)
        result = self.ledger.finish()
        self.assertEqual(result["held_at_stop"], 1)
        self.assertEqual(result["complete_cycles"]["rc003"]["back"], 1)

    def test_stop_is_reset_not_a_synthetic_release(self):
        self.state(["back", "volume_up", "volume_down"])
        result = self.ledger.finish()
        self.assertEqual(result["held_at_stop"], 3)
        self.assertEqual(len(self.events), 3)
        self.assertEqual(sum(result["complete_cycles"]["rc003"].values()), 0)

    def test_ordinary_keyboard_usages_not_accepted(self):
        self.state(["F13"])
        self.state(["mic"])
        self.assertEqual(self.events, [])

    def test_hardware_match_is_exact(self):
        for suffix in ("", "_012345abcdef"):
            self.assertIsNotNone(capture.HARDWARE_RE.fullmatch(capture.HARDWARE.upper() + suffix))
        for suffix in ("0", "_", "_012345abcdef0", "_012345abcdeg", "&col01", "\\other"):
            self.assertIsNone(capture.HARDWARE_RE.fullmatch(capture.HARDWARE + suffix))

    def test_no_implicit_elevation_or_attachment(self):
        with patch.object(capture, "is_admin", return_value=False):
            with self.assertRaisesRegex(capture.ProbeError, "administrator_launch_required"):
                capture.capture(None, None, None)

    def test_no_driver_overlap(self):
        with patch.object(capture, "is_admin", return_value=True):
            with patch.object(capture, "security_state", return_value={"experimental_filter_present": True}):
                with self.assertRaisesRegex(capture.ProbeError, "do_not_stack_filter_and_tap"):
                    capture.capture(None, None, None)

    def test_no_frida_import_during_tests(self):
        self.assertNotIn("frida", sys.modules)

    def test_windows_token_privilege_layout(self):
        self.assertEqual(ctypes.sizeof(capture.Luid), 8)
        self.assertEqual(ctypes.sizeof(capture.TokenPrivilege), 16)
        self.assertEqual(capture.TokenPrivilege.attributes.offset, 12)

    def test_gadget_authentication_requires_token_and_target_pid(self):
        token = "a" * 64
        self.assertTrue(gadget.authenticated({"kind": "hello", "pid": 23, "token": token}, token, 23))
        for message in (None, [], {}, {"kind": "hello", "pid": True, "token": token},
                        {"kind": "hello", "pid": 24, "token": token},
                        {"kind": "hello", "pid": 23, "token": "b" * 64},
                        {"kind": "hello", "pid": 23, "token": chr(0x100) * 64},
                        {"kind": "hello", "pid": 23, "token": [token]}):
            self.assertFalse(gadget.authenticated(message, token, 23))


if __name__ == "__main__":
    unittest.main(verbosity=2)
