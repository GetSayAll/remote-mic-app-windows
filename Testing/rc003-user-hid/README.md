# RC003 User-Mode HID Diagnostic And Optional Bridge

This is an optional experiment, separate from SayAll's basic voice path. The
standalone diagnostic never runs mapping actions. The optional packaged bridge
connects the three captured states to SayAll's existing mapping engine after
explicit consent and per-session physical-key calibration. Only a real RC003
test can establish whether mapped actions work on a particular Windows build.

> **Concurrent-use compatibility note:** This optional helper loads Frida into a
> Windows Bluetooth HID host and may not run normally alongside game anti-cheat
> software. Compatibility is untested. Competitive PC gaming users should be aware
> of this possible coexistence limitation. This is not a claim that a game will
> classify its use as cheating; we have no such test result.
> Ordinary mouse actions do not require this helper. See the
> [local extension notes](../../docs/local-extensions.md).

## Boundaries

- `--probe` reads public Windows device information; it never imports Frida.
- `--capture` requires an explicitly elevated helper. The regular SayAll app
  stays unelevated and its BLE, audio, F5, and settings are not modified.
- The helper attaches only to the one live RC003 REV 00A4 HID host discovered
  through Windows, verifies the system WUDFHost executable and Microsoft
  signature, and stops if its host/device changes. Arbitrary PIDs are not accepted.
- The elevated helper enables its own existing `SeDebugPrivilege` with
  `AdjustTokenPrivileges` and restores the prior token state on cleanup.
  It does not change account rights or system policy. Being administrator alone
  does not mean this token privilege is already enabled.
- No kernel drivers, test signing, Secure Boot changes, Defender exclusions,
  autostart, services, externally reachable servers, or keyboard injection are installed.
- Frida still injects native code into WUDFHost. A bug may disrupt Bluetooth.
  Game anti-cheat compatibility is untested; avoid concurrent use with games
  running anti-cheat. Do not bypass protection or repeatedly force a denied attach.
- The in-host hook queries each I/O handle's current NT object. Other devices
  are rejected before reading buffers. A UMDF proxy is always labeled
  `proxy_unverified`; it is not automatically attributed to RC003. Proxy
  diagnostics are disabled if another live BLE HID service shares the host.
- Only the three target button states leave the host. No raw buffers, other
  keyboard usages, voice data, device identities, or paths enter diagnostic logs.
- Only completed synchronous 9-byte IOCTL results are parsed. Pending I/O is
  counted but its buffer is never read later. This may miss a Windows variant;
  zero captures is not proof the remote never sent the buttons.
- This host returns success with a 9-byte declared output and
  `IO_STATUS_BLOCK.Information=0`. This was measured before relaxing the
  exact-written-length check. Only 0 (unspecified) or 9 is accepted, the status
  must indicate success, and the `01 00 00` header remains mandatory. No partial
  buffer, pending completion, or unknown-length allocation is read.
- The standalone diagnostic's host-side deadline removes the hook after 15-180 seconds. Normal cleanup
  stops the hook, unloads the script, and detaches the Frida session. No Gadget
  DLL with an automatic reconnect loop is installed. API success is not a
  guarantee that every Frida runtime module has been unloaded from Windows.
- Optional `-Backend gadget` uses the original project's DLL-loading technique
  when ordinary Frida attachment cannot establish communication. A loopback
  receiver exists only during the diagnostic and requires a random per-run
  256-bit token and matching host PID. The injected code makes one outbound
  connection and stops on EOF; there is no network listener in WUDFHost.
- Gadget is verified against both official archive and DLL SHA-256 values.
  It is placed under a diagnostic-only ProgramData folder writable only by
  Administrators/SYSTEM and readable by LocalService. Per-host PID/start-time
  subdirectories prevent a later test from waking an old host's script.
  The DLL and its file watcher remain resident after capture, but the hook
  stops. Only an explicit elevated run updates the watched script to start a
  new test. No boot registration is created; full DLL removal requires Windows
  to recycle the host. Do not claim that stopping capture unloads Gadget.
- Disconnection, expiry, and cancellation reset observations, not physical
  UP edges. The helper never claims hardware verification from synthetic data.

## Run

Requires Python >= 3.11 x64. From this directory in PowerShell:

```powershell
.\prepare.ps1
.\prepare.ps1 -Gadget
.\run.ps1 -Mode Probe
.\run.ps1 -Mode Inspect
.\run.ps1 -Mode Capture -Seconds 90
.\run.ps1 -Mode Capture -Backend gadget -Seconds 90
.\run.ps1 -Mode Status
.\run.ps1 -Mode Stop
```

Preparation creates only a local ignored virtual environment and installs the
official PyPI Frida 17.15.3 Windows x64 wheel with a pinned SHA-256. It does not
attach. Capture alone prompts for UAC. It runs hidden, writing `.runs/*.jsonl`.
Stop requests cooperative cleanup; it never kills WUDFHost or SayAll. Logs and
the environment are excluded from Git. No startup registration is created.

After `capture_ready`, press and release Back, Volume Up, and Volume Down three
times each, separately. Leave a short gap between keys. Then test ordinary
keyboard keys and a brief normal voice input. Pressing those keys in software
cannot validate the physical RC003. Review DOWN/UP pairs per scope/stream,
hook cleanup, host health, and the existing SayAll voice log before integration.
Any proxy-only observation still needs reliable device attribution; do not
claim direct attribution simply because its three usage codes look plausible.
The bridge remains explicitly experimental and subject to the additional
session gates below, not a generally verified per-device input backend.

## Optional SayAll Integration

The local bundle includes `sayall-rc003-helper.exe` and its private runtime.
Enable RC003 enhanced capture in the mapping page, accept the risk dialog, and
approve Windows UAC. The main app stays unelevated. Nothing starts at login or
automatically after disconnect/reconnect. Existing saved mappings are preserved.

- Startup requires a connected RC003, exactly one matching Raw Input device,
  one live HID service in the discovered host, and an exact match to the BLE
  peer selected in SayAll. Peer identity remains in memory and is not logged.
- App/helper messages use a random per-session 256-bit token over loopback,
  strict bounded JSON, ordered sequence numbers, and only three named states.
  The helper does not call SendInput or accept arbitrary commands or PIDs.
- The first complete press/release for each key confirms that key for this
  session without running its mapping. Subsequent edges use the existing
  single/double/hold mapping engine. The UI labels proxy scope as experimental;
  it does not reuse the driver-confirmed badge or manufacture F13-F15 events.
- App leases arrive every second. Five seconds without an app lease stops the
  helper; the independent in-host lease expires within 15 seconds without
  renewal. Stop, connection changes, source changes, invalid input, and missing
  heartbeats invalidate mapping permission and clear session confirmation.
- Queued states older than two seconds are rejected. Sustained input above
  256 states/second or a target key held for ten seconds stops this experiment.
  These limits do not apply to the voice key or modify its existing timing.
- Stop disables mapping permission immediately and requests cooperative hook
  removal. A cleanup acknowledgement does not mean the Gadget DLL unloaded.
  Do not use this feature alongside anti-cheat software or shared-host devices.

Build on Windows x64 with Python 3.12, the repository Rust/Node toolchain, and
the already hash-verified official Gadget archive:

```powershell
.\build-helper.ps1 -Prepare
# From the repository root:
pnpm tauri build --bundles nsis --config Testing/rc003-local-bundle.json --ci --ignore-version-mismatches
```

Build dependencies are pinned with SHA-256 in `requirements-build.txt` and
installed only into an ignored build virtual environment. The helper bundle
includes its corresponding source, build scripts, GPL/Frida notices, and
Python/PyInstaller runtime notices. Only the explicit local bundle override
includes this experiment; the normal release configuration stays unchanged.

## Tests And Status

```powershell
python -I test_capture.py
python -I test_bridge.py
node test_hid_tap.cjs
node test_gadget_adapter.cjs
```

Offline tests use isolated protocol fixtures and never attach. There are 34
passing cases. RC003 capture and per-key confirmation have been exercised with
an unverified proxy source. Full physical mapping, device isolation, idle-first
input, reconnect and voice acceptance remain incomplete. See
[the acceptance guide](../WindowsInputExtensions.md) and
[the implementation](../../docs/architecture/rc003-enhanced-capture.md), including
the unresolved coalesced-handshake issue. Offline tests are not hardware or
anti-cheat compatibility certification.

## Source And License

This diagnostic/helper directory is GPL-3.0-only, a separate process from the normal app.
Protocol and hook scoping are adapted from `leowzz/axonkey`, commit
`a0451ec59cbcb48063f8dc1d7104f817d3cbaff6`, modules
`tools/keycode-demo/rc003_hid/frida_hid_tap_runtime.py`, `frida_compat.py`, and
`__main__.py`. Copyright (C) 2026 Remote Mic contributors; upstream credits
`ZSTDJan/windows-remote-mic-app` at
`ca1d4946a4336ba517e4e2c633f21077ceae82d9` and `remote-bridge-hub`.
The full upstream license is retained in `LICENSE-GPL-3.0.txt`.

Initial transport: explicit Frida Python session instead of Gadget/TCP, bounded
capture, exact live target checking, Microsoft host signature verification,
no implicit elevation, no auto-reattach, IO_STATUS_BLOCK completion checks,
three-key-only redacted diagnostics, and separate unverified proxy states.

The alternative Gadget backend additionally adapts the upstream
`frida_hid_tap_injector.py` LoadLibrary sequence, with per-process loader RVA
resolution, exact loaded-module checking, protected per-host runtime files,
authenticated loopback, a bounded handshake, and no automatic reconnect.
Gadget's unchanged dependency license is in `LICENSE-Frida.txt`.

Frida is an unmodified official dependency, separately installed for diagnostic
mode and bundled as Gadget for the optional bridge. Its notices are retained.
Official documentation:
`https://frida.re/docs/javascript-api/`; Python API source:
`https://github.com/frida/frida-python/tree/17.15.3`.
Packaging reference: `https://pyinstaller.org/en/v6.16.0/usage.html`.
