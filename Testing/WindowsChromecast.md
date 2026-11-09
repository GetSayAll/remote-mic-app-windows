# Google Chromecast Windows adapter — experimental

Scope: Google Chromecast Voice Remote, VID `18D1` / PID `9450`. Other Google TV devices and compatible remotes are not claimed. This is a local development branch, not a release or hardware acceptance record.

## Chrome dictation preview

Build with `pwsh -File scripts/build-chromecast-preview.ps1` from an x64
MSVC Developer PowerShell with Rust, the Windows SDK and lockfile-pinned pnpm
dependencies available. Successful builds produce a new EXE and
`build-metadata.json` under `target/chromecast-local-preview/`. The settings
identifier is `app.getsayall.remote-mic.windows.chromecast-test`. Diagnostic
logs are under `%LOCALAPPDATA%\SayAllChromecastPreview\Logs`.

1. Exit regular SayAll using its tray menu. Launch the preview, which shares
   the hardware single-instance lock and explains a conflicting launch.
2. Select the paired Chromecast and route SayAll playback to `CABLE Input`.
   Set WeType's microphone to `CABLE Output` and check the WeType voice shortcut
   against the SayAll preset.
3. Click inside the intended Chrome draft, then hold the remote's voice button,
   speak and release. Wait for the finished text before changing focus or tabs.
   Review the draft and send using the page's normal controls. System dictation
   follows the foreground input. This trial uses a single selected draft, with
   no site-specific integration.
4. Use artificial utterances with Chinese, English and sample code identifiers.
   Test away from production sessions. Note the exact artifact digest, Windows,
   Chrome, WeType and firmware versions, phase/error codes, first audio and
   release-to-final-text times. Keep voice recordings, transcript content and
   device paths out of the report.

If a trial fails, record the last successful phase (connection, first PCM,
input method listening, or text in the correct draft) and the failure phase.
Use the tray to exit the preview before starting another build or regular
SayAll. The portable preview does not perform an in-place update.

## Implementation

- The paired-device scan recognizes `Chromecast Remote`, exposes `chromecast` through IPC, and reuses SayAll's WinRT GATT subscription, generation filtering, reconnect, shutdown and WASAPI output paths.
- Chromecast allows the documented ATVV 0.4 and 1.0 capability combinations. Version 1 notifications decode immediately; a sync before stream start survives until first audio. Version 0.4 validates each frame header before decoding, and 8 kHz PCM is duplicated to the existing 16 kHz output path. This simple rate conversion needs listening acceptance. Xiaomi retains its existing frame policy.
- Raw Input observes Google reports only when the pinned VID/PID and the selected ready Bluetooth target agree. It recognizes report 1 button indices and report 2 little-endian consumer usages. Google observations do not trigger mappings or macros. Windows can still perform native actions on these keys.
- Chromecast uses separate start and first-PCM deadlines. Recovery closes the failed session and invalidates its callback generation, so the next recording requires another press. Xiaomi F5 suppression is scoped to the Xiaomi path.
- Xiaomi's optional enhanced capture helper is unavailable for Chromecast. The enable command rejects it, startup reconciliation excludes it, and the UI disables the control. Do not claim full-key support or transplant the Xiaomi helper's hardware selector into a Google device.
- The diagram is an original SVG schematic; no vRemoter branding or pictures were copied.

## Historical automated evidence (2026-11-05)

Standard: LT-TEST-001 1.0.0. Native Rust protocol/unit tests and Vue component/contract tests are the applicable first layers. Existing runners are retained; no shared runner was copied. The shared UI kit is not integrated by this change. Vue tests use jsdom and mocked native IPC; they do not prove native behavior.

- Frontend typecheck + production build: passed.
- Initial targeted frontend checks: 3 files / 142 tests passed. Final full suite: 37 files, 595 passed / 10 skipped. Two obsolete Xiaomi-only copy assertions failed on the first full run and were updated to the actual new copy before the passing rerun.
- Rust formatting: passed. Protocol tests: added for both audio versions, sync ordering, packet validation, report identities and shapes. Compilation first failed because `link.exe` was absent; after importing the newly installed MSVC environment it reached the linker but failed on missing `kernel32.lib` (Windows SDK not yet available). These are environment build failures, not passing Rust tests. Workspace type checking and core tests remain unverified.
- No Computer Use, OS keystroke injection, audio playback, Bluetooth pairing or connection was performed by automated regression.
- EXE build/install, installed lifecycle and all native-device cases below remain deferred until the C++ toolchain is ready and this remote is paired.

## Required native acceptance

1. Pair manually with Windows Bluetooth settings; hold Back + Home to enter pairing mode. Record model/firmware and protocol version without logging device addresses.
2. Select the remote explicitly; verify capabilities and connected model. Start/stop quickly, hold longer than 10 seconds, and verify PCM arrives at the selected CABLE endpoint and the input tool produces text in the intended draft.
3. Test cold first press, 30 repeated dictations, disconnect while speaking, reconnect, sleep/wake, exit while held, and stale-generation notifications. Verify no stuck modifiers or lost draft.
4. Test each physical button and release, including direct transitions and repeat. Record which report format reaches Windows Raw Input and whether native actions also occur. Check that Google observations never execute a SayAll mapping or macro and that physical keyboard F5 still reaches Chrome.
5. Verify unknown firmware/report IDs are rejected, two connected remotes remain isolated, and Xiaomi RC001 / RC003 regression still passes independently.
6. Enhanced per-device capture requires a separately reviewed Google implementation and native acceptance; it is not implemented here.

## Chrome preview evidence (2026-11-09)

- Vue: 598 passed, 10 skipped. Type checking and production build passed.
- Windows MSVC: 11 Chromecast-focused tests passed, covering capabilities,
  device identity, timeout stages and F5/keyboard isolation. The mapping
  engine's 31 tests also passed using simulated input.
- Build validation checks the 19 embedded frontend assets and records the EXE
  hash and Windows signature status in `build-metadata.json`.
- A stopped PCM stream does not tell us whether WeType already committed text.
  Only observing the draft counts as successful delivery. With ATVV 0.4,
  long-hold behavior and any need for a keepalive remain native test cases.
- Native Chrome/WeType dictation, paired-device and power recovery, Xiaomi
  cold F5 and enhanced helper retarget timing, and signed installation and
  update lifecycle are deferred. Use an isolated session and exit gracefully.

Current verdict: **development prototype; native functionality unverified**.
