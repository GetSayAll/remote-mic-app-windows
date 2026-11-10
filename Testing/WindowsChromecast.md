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

## Configure and verify ordinary buttons

1. Connect the Chromecast Voice Remote and open the button configuration page.
   The image and selectable keys should match the selected remote, including
   when returning from a temporary disconnect.
2. Enable button mappings. Pick an ordinary key and a gesture, then select
   the keyboard shortcut action. For example, set single click to
   `Shift+Enter` and double click to a different shortcut. Test with a
   disposable draft that can show the shortcut's effect without posting.
3. For a key with triple click configured, verify one tap executes only its
   single action, two taps execute only its double action, and three taps
   execute only its triple action. Test a long hold and release separately,
   including the hold-repeat option for a repeatable action.
4. Test each physical key's down and release, plus any original Windows action.
   A button that sends only infrared or produces no observable HID report
   cannot run a SayAll mapping. Windows may also execute the native action for
   a Bluetooth key. Record the key name, gesture, result and native effect
   without including any device paths.
5. Restart the preview, check the Cast mappings persist, and switch between
   the Cast and Xiaomi remotes to confirm that their button settings stay
   separate and that queued gestures are discarded on a device change.

## Implementation

- The paired-device scan recognizes `Chromecast Remote`, exposes `chromecast` through IPC, and reuses SayAll's WinRT GATT subscription, generation filtering, reconnect, shutdown and WASAPI output paths.
- Chromecast allows the documented ATVV 0.4 and 1.0 capability combinations. Version 1 notifications decode immediately; a sync before stream start survives until first audio. Version 0.4 validates each frame header before decoding, and 8 kHz PCM is duplicated to the existing 16 kHz output path. This simple rate conversion needs listening acceptance. Xiaomi retains its existing frame policy.
- Raw Input accepts Google events only when the pinned VID/PID and the selected
  ready Bluetooth target agree. It recognizes report 1 button indices, report 2
  consumer usages and supported keyboard events. A key uses one report source
  per target session so duplicate delivery cannot turn one press into a multi-click.
- Ordinary Google keys use the shared gesture and action engine with an empty,
  independently persisted default mapping. Triple click adds a 300 ms interval
  between clicks only for keys with that action configured. A disabled mapping
  leaves the original event to Windows. Windows may
  still perform the original action, such as Enter on the center button, so
  check the native effect before relying on a mapping in a real draft.
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
4. Run the ordinary-button matrix above. Check that disabled Cast actions generate no synthetic key and that physical keyboard F5 still reaches Chrome.
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

## Later local device evidence (2026-11-09)

The prior preview reached ATVV Ready on a reconnect, matched one HID
interface, enqueued PCM on repeated presses, and passed the first-run wizard.
The user reported voice input was available. This confirms the basic device
path on this PC. The previous build had an observation-only button path,
so these records do not establish that any ordinary key runs a mapping.

## Three-byte Report 1 and physical key feedback

- The failing preview saved the enabled center-key mapping but logged no
  gesture or dispatch. The present Windows Consumer Control collection
  exposes a 3-byte input report, ID 1, with usage `0x0041` for OK. The old
  decoder only accepted ID 1 with 2 bytes.
- On an isolated test worker, Windows' HID parser synthesized OK down/up from
  the actual installed descriptor. The production report parser and mapping
  engine produced exactly one recorded `Shift+Enter` and a pair of button
  events. No system input was generated. Run
  `cargo test --locked -p sayall-windows --lib chromecast_native_descriptor_ok_dispatches_shift_enter_and_releases_highlight -- --ignored --nocapture`
  with the device's HID collection present.
- The event JSON printed after `CAST_E2E_EDGES=` was passed unchanged to the
  Vue contract test using `SAYALL_CAST_E2E_EDGE_FIXTURE` with its file path.
  Run `pnpm exec vitest run src/pages/ButtonsPage.cast-contract.test.ts` to
  exercise Tauri's JavaScript event adapter and the actual Vue page with a
  stubbed native endpoint. It verifies center and row illumination, dark
  neighbors, and release. The standalone page suite covers all 14 ordinary
  keys and clearing on disconnect or device change.
- Windows Rust serial regression passed 542 tests with 17 ignored. The
  present-descriptor test passed separately. The active desktop, BLE settings,
  physical radio key transmission and Codex shortcut receipt remain outside
  this test evidence.

Product comparison and the local Whisper boundary are recorded in the
[capability review](../docs/investigations/2026-11-09-sayall-whisper-capability-review.md).

## Ordinary-button diagnostics (2026-11-10)

The `303d3f6` local preview logged a ready voice session, repeated PCM and one
matched Raw Input collection. The persisted OK single-click action was `Enter`,
with mapping enabled. No button gesture or dispatch appeared in that run. The
older log could not distinguish collection enumeration from Windows delivering
an actual packet to SayAll.

The preview writes to `%LOCALAPPDATA%\SayAllChromecastPreview\Logs\sayall-diagnostic.log`.
Read these events in order for the same `generation`:

1. `action=button_flow` starts with an audit of the matched collection count.
   Subsequent changed audits distinguish `accepted_packets` from
   `rejected_paths`, which counts recognized Chromecast paths rejected for the
   current target. `action=first_button_packet` records the first accepted
   packet immediately. Voice readiness is logged separately.
2. `hid_reports`, `decode_failures` and `keyboard_unmapped` identify what the
   listener could decode. `engine_forwarded` counts reports queued to the
   mapping worker. Compare those counts with `map_edges model=chromecast` to
   confirm the worker produced button down/up edges. Identical reports and
   reports from a superseded target may produce no edge.
3. The edge log includes `suspended`. For an active mapping, `map_gesture`
   gives the resolved trigger and `map_dispatch` distinguishes disabled or
   unassigned mappings from a rejected or queued shortcut.
4. `shortcut_queued` means the injection backend accepted the shortcut.
   `global_hook_echo_delta` counts process-wide hook activity during that
   window. Confirm receipt separately in the focused application.

The events omit HID paths, Bluetooth addresses and report payloads. Ordinary
keyboard input is excluded from the Chromecast counters and edge detail.

### Listener restart and preview activation

An earlier listener owned a one-use mapping sender. Stopping and starting the
listener again made it look ready while dropping every message sent to the
mapping engine. A Windows regression starts and stops the message-only
listener twice and verifies that both stops reach the same engine channel.
The test uses no physical keys or system input injection.

Check `source_revision` and `pid` in the startup and input records before
comparing runs. Opening a second EXE while an old instance owns the shared
lock leaves the old listener active. For an existing, configured preview,
`--diagnostic-background` starts the replacement in the tray without changing
its saved startup preference. In this mode a lock conflict is logged without
opening a dialog.

### Shared HID input for the selected Chromecast collection

On the running `947a0e6` preview, the user pressed OK after dictation. The
log showed the voice stream reaching PCM, one matched Consumer Control
collection, and zero accepted or rejected Raw Input packets. The saved action
was `Enter`. The mapping engine had no press to act on.

A Windows probe verified that the exact consumer collection can be opened
read-only with shared access, and that a pending overlapped read can be
cancelled and reaped. The preview can read report 1 through that collection
and through Raw Input. The first decoded press chooses one transport for
report 1 until the target generation changes. If its collection is lost, the
engine cancels pending gestures and the same transport must report a release
before it can dispatch again. Separate counts record window-delivered
reports and `direct_reports`; the transport
selection is logged with its target generation. Verify physical press and
release and the focused application's response separately.

## One-shot local Whisper dictation (2026-11-10)

In Reflections, open Local Whisper Transcription, select the verified Whisper
CLI and model, and arm the next Chromecast utterance. The preview keeps the
armed capture in memory. A normal stop hands decoded 16 kHz mono PCM to the
local engine, while cancellation, disconnect or an expired capture discards
it. The completed text is an editable SayAll draft. Copying it is explicit.
Optional local terminology rules apply literal replacements and spoken text
snippets, with one-step undo while the modified draft remains untouched.

- The Windows native capture regression feeds synthetic ATVV into the decoder
  and verifies a single PCM handoff, timeout, bounds and stale-session isolation.
- The host smoke test feeds synthetic Mandarin PCM to the real pinned Whisper
  v1.9.1 x64 CPU engine and multilingual `base-q5_1` model. It checks the ready
  draft and temporary WAV cleanup. A 5.6-second fixture transcribed in about
  1.1–1.5 seconds after engine preparation, with one recognition error.
- The frontend regression covers cancellation, late results, existing draft
  edits and terminology apply/undo. It uses simulated IPC.
- Physical remote timing and same-subscription ATVV ordering are native
  acceptance cases. Chrome draft binding and Codex shortcut receipt have
  separate acceptance criteria.
