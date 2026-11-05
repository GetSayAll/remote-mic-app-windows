# Google Chromecast Windows adapter — experimental

Scope: Google Chromecast Voice Remote, VID `18D1` / PID `9450`. Other Google TV devices and compatible remotes are not claimed. This is a local development branch, not a release or hardware acceptance record.

## Implementation

- The paired-device scan recognizes `Chromecast Remote`, exposes `chromecast` through IPC, and reuses SayAll's WinRT GATT subscription, generation filtering, reconnect, shutdown and WASAPI output paths.
- Chromecast enables a separate ATVV decoding policy. Version 1 notifications decode immediately; a sync before stream start survives until first audio. Version 0.4 validates each frame header before decoding, and 8 kHz PCM is duplicated to the existing 16 kHz output path. This simple rate conversion needs listening acceptance. Xiaomi retains its existing frame policy.
- Raw Input accepts the pinned Google VID/PID, decodes report 1 button indices or report 2 little-endian consumer usages, and uses the existing gesture engine. YouTube, Netflix and Input have distinct semantic keys. Voice is excluded from button mappings.
- Google input never arms Xiaomi keyboard suppression or F5 recovery. The public Raw Input path cannot suppress all native consumer actions. A custom mapping may also produce the original action. Disabled actions must not replay native keys.
- Xiaomi's optional enhanced capture helper is unavailable for Chromecast. The enable command rejects it, startup reconciliation excludes it, and the UI disables the control. Do not claim full-key support or transplant the Xiaomi helper's hardware selector into a Google device.
- The diagram is an original SVG schematic; no vRemoter branding or pictures were copied.

## Automated evidence (2026-11-05)

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
4. Test each physical button and release, including direct transitions and repeat. Record which report format reaches Windows Raw Input and whether native actions also occur. Do not call remapping accepted if a needed usage never reaches Raw Input.
5. Verify unknown firmware/report IDs are rejected, two connected remotes remain isolated, and Xiaomi RC001 / RC003 regression still passes independently.
6. Enhanced per-device capture requires a separately reviewed Google implementation and native acceptance; it is not implemented here.

Current verdict: **development prototype; native functionality unverified**.
