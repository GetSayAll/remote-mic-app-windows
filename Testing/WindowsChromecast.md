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
cancelled and reaped. The direct reader and Raw Input both support the known
report 1 and report 2 formats. The first decoded press chooses one transport
per report ID until the target generation changes. If its collection is
lost, the engine cancels pending gestures and the same transport must report
a release before it can dispatch again. `kernel_completion` records the
first completed direct read before format filtering, and logs each new report
shape. Separate counters track Raw Input and decoded `direct_reports`.

During the `2f7c850` physical retry, mappings saved successfully and the voice
stream reached PCM. There were no forwarded ordinary button reports or mapping
edges. That build silently skipped non-ID-1 direct reads, so its log cannot
rule those out. A read-only Windows parser check on the installed Consumer
Control collection accepted input report 1 and returned
`HIDP_STATUS_REPORT_DOES_NOT_EXIST` for input report 2. The additional format
support covers another documented firmware layout.

Xiaomi ordinary keys use the existing device-attributed Raw Input stream.
Chromecast shares its gesture and shortcut engine, but needs its own source
of ordinary-button edges. The optional Xiaomi enhanced capture helper is
device-specific. A separate Google capture path would need a real report
trace before it can claim to support the installed device. See CAST-12 in
[the backlog](../TODO.md). Verify the focused application's newline behavior
after confirming receipt of an actual shortcut.

### Mapping editor startup and maximize

In the `e5fa92c` desktop run, the first main document tried state-backed IPC
before Tauri registered application state. Mapping cells remained disabled
and the initial snapshot rejection interrupted page setup. The main WebView
now loads after state registration. Its `document_load` diagnostic includes
`state_managed`. Mapping read failures expose a retry, and canvas geometry
tracks viewport changes even when an IPC call fails. Cards, their arrows and
the voice card use the same measured canvas width.

Click any of the 14 ordinary keys in the illustration or its card to edit the
single press. The individual cells edit the corresponding gestures. A
renderer regression verifies saving a shortcut during a remote reconnect.
Headless Chrome checked pointer hit testing and all 15 arrow-to-card
connections at viewport widths of 1100, 1950 and 1280 pixels. These checks
cover the editor and layout. Physical delivery of ordinary button reports
is tracked independently in CAST-12.

The subsequent `5a47241` field retry recorded four submitted shared HID
reads, zero kernel completions, zero Raw Input packets for the selected
Chromecast, and zero mapping edges or actions across four launches. The
same sample contained 28 first-PCM events and a successful mapping save.
Because this build logs kernel completion before report filtering, the
failure occurs upstream of the SayAll decoder and mapping engine. This
evidence leaves the Bluetooth radio and Windows transport boundary open.

A cache-first WinRT query for the selected device found its HID-over-GATT
service (`0x1812`). The follow-up characteristic query returned `AccessDenied`,
so this path could not even enumerate the input report characteristic.

For CAST-12, use an isolated Windows test host and capture the selected
remote's connection with Microsoft's Bluetooth Virtual Sniffer. Correlate
the connection and input report characteristic before testing one OK press
and release without dictation, then one after dictation. Record only the
sanitized report count and whether both edges occurred. If the trace shows
reports that the public HID interfaces do not deliver, test a read-only
Google-specific report-layer helper with exact device-instance and handle
attribution. If the trace has no input notification, check the remote's
report mode and HID notification subscription before working on a helper.

### Comparing Xiaomi and Chromecast diagnostics

The sanitized local collection covered the six regular SayAll files and the
Chromecast preview log. The Xiaomi-era regular build (`f3f5212`) recorded 850
semantic-edge events and 322 OK shortcut dispatches. Its old records did not
label each event's model. The `5a47241` Chromecast sample had 41 first-PCM
events, five shared-HID submissions, zero kernel completions and zero semantic
edges. The samples cover different times, so the counts establish observed
stages rather than a rate comparison. Present Xiaomi and Chromecast HOGP
parent nodes both used `mshidumdf` version `10.0.26100.9549` and showed no PnP
problem. A normal node is not evidence of report delivery.

The RC003 acceptance trace provides a more specific control. Two OK presses
arrived as two `VK_RETURN` keypresses via Raw Input and Home arrived once as
`VK_HOME`. Enhanced RC003 capture addressed Back and Volume +/−, while
ordinary OK kept its standard Windows route. See the
[RC003 controlled capture](../hardware/RC003/README.md).

In a fresh run, read the following records for the same process and target
generation:

1. `remote_target` identifies the target model, generation, and whether the
   Xiaomi helper target was requested. `registration_readback` identifies
   whether Windows registered each Raw Input collection to the listener.
2. `window_flow` counts messages and failures before attribution. It counts
   deliveries to the listener as a whole. `button_flow` then counts only the
   selected model's routed reports, decode results, and forwarding outcomes.
3. For Chromecast, `direct_reader_binding` links the generation to a
   process-local `reader_id`. `shared_hid_open` describes the verified
   collection, and `shared_hid_read` records submission, bounded pending
   snapshots, kernel completion before decoding, and terminal counters.
4. `shortcut_settings` identifies the configured model, `map_edges` and
   `map_gesture` show decoded input, `map_dispatch` gives the gate outcome,
   and `map_shortcut` records backend acceptance and elapsed time. Confirm
   the focused application's response independently.

The added diagnostics record aggregate counts, stable button and gesture
labels, and numeric error codes. They omit raw packets, keyboard input text,
device addresses, interface paths, audio, and application window titles.

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

## Vendor key transport — HCI capture verdict (2026-11-10)

Microsoft Bluetooth Virtual Sniffer (BTP 1.14.0, `btvs.exe -Mode Wireshark`)
captured the live HCI stream while the preview was connected. Two labeled
button sessions plus one voice press were recorded; the pcap evidence is
summarized below (packet payloads sanitized to codes and handles only).

- **The remote never sends HID input reports to this Windows host.** Across
  the entire capture the only notifying handles were battery (`0x001d`,
  level 99–100), the key-event stream (`0x0029`), ATVV audio (`0x003c`) and
  ATVV control (`0x003f`). Raw Input and the shared HID reader observing zero
  packets is therefore expected and permanent for this pairing; the CAST-12
  idea of a Google report-layer Helper has nothing to capture.
- **Ordinary keys ride the vendor key service.** Every press produced one
  8-byte notification on handle `0x0029` — first byte key code, `0x00` for
  release — always paired with a `StreamStopped` (`[00 02]`) control
  notification. A 19-press labeled session mapped the codes exactly onto the
  legacy report-1 index table already decoded in `raw_input.rs` (1=Power,
  3=Up, 4=Down, 5=Left, 6=Right, 7=OK, 8=Mute, 10=Home, 11=Back, 12=Vol+,
  13=Vol−, 14=YouTube, 15=Netflix, 17=Input). The voice key is not on this
  channel; it uses the ATVV control stream as before.
- **Service identity.** Windows enumerates the notify-capable characteristic
  as `D343BFC5-5A21-4F05-BC7D-AF01F617B664` inside service
  `{D343BFC0-5A21-4F05-BC7D-AF01F617B664}` (same UUID base as ATVV, five
  characteristics, exactly one notify-capable). The ATVV service itself
  exposes only transmit/audio/control, and transmit (`0xAB5E0002`) is
  write-only per Windows properties — subscribing it fails with
  "特征不支持 Notify 或 Indicate".

### Implementation state

Commit `fe84680` discovers the key service (uncached), picks the
notify-capable characteristic by property (excluding the known ATVV UUIDs,
falling back to the ATVV service for future firmware), subscribes it as a
`Transmit` worker channel, decodes the 8-byte reports via
`decode_chromecast_vendor_key`, and forwards edges to the mapping engine as
a `Vendor` source with the existing per-key single-source arbitration.
Session cleanup unsubscribes and closes the retained service
(service+characteristic+token triple, mirroring the battery subscription —
dropping the `GattDeviceService` kills WinRT notification routing).

### Open firmware question (blocks live button acceptance)

Before any host CCCD write, the remote streamed key notifications
unconditionally (nobody was subscribed during the captures). After the first
explicit `CCCD=Notify` write on `D343BFC5`, key notifications stopped while
control/audio kept flowing: the app then observed only `[00 02]` shadows per
press and zero `direction=K` packets. Suspected firmware behavior: the CCCD
write (or the app-restart churn around it) disables the default-on notify.
Next evidence steps: re-pair the remote with a capture running to reset the
bond store and observe the fresh default, then decide between a listen-only
subscription (register `ValueChanged` without writing the CCCD) and
re-writing the CCCD after connect. Native button acceptance (CAST-09 matrix)
remains blocked on this question.

### Cache-mismatch closure (2026-11-10, evening)

The full-day evidence chain, final state:

- The remote streams key notifications on over-the-air handle `0x0029`
  unconditionally (28 further notifications captured at 20:08 while no host
  subscription mapped to that handle).
- Windows subscribes our candidate at CCCD `0x0037` (cached table), receives
  the `0x0029` notifications at the stack level (btvs sees them on the same
  connection as the app's audio/control traffic) and drops them — no WinRT
  `ValueChanged` is ever raised. The `Uncached` queries never hit the air
  (zero ATT discovery in every captured reconnect), so the property table
  cannot self-correct from inside the app.
- Widening the subscription to every Notify/Indicate candidate of the key
  service does not help: the cached table marks exactly one characteristic
  (`D343BFC5` at `0x36/0x37`) as notify-capable and has no notify entry at
  the live key handle.

**Verdict for this machine:** a Windows GATT property-cache/table mismatch
localized to the key-event service (ATVV and battery handles all match the
live table). No public WinRT API forces re-discovery, and raw handle-level
ATT is not reachable from an app. **User-side reset:** remove the device,
restart Windows (a radio toggle or re-pair alone did not rebuild the table),
then re-pair — a fresh pairing builds the cache from the live attribute
table, where the subscribed candidate should be the real key handle. This
reset and a post-reset capture remain the outstanding verification step;
the pcap evidence lives at
`artifacts/cast-vendor-key-evidence-2026-11-10.pcap`.
