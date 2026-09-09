# Audio interrupt freezes the application after launch

- Discovered: 2026-09-09.
- Status: fixed; Windows worker regression and application startup acceptance passed.
- Scope: Windows v0.2.3, introduced by production diagnostics in df63e5e.
- Trigger: launch with a saved remote. Restore schedules a BLE connection, whose cleanup first interrupts audio.
- Expected: audio cleanup replies and the initial runtime snapshot renders.
- Evidence: five existing launch traces stopped after Vue mount without initial IPC completion. A fresh launch also reported Responding=False. The real audio-worker regression timed out after two seconds on the original implementation and completed immediately after the fix.
- Cause: AudioMessage::Interrupt passed two lock(&state) guards to one format! expression. Rust retained the first temporary guard until the statement ended, so acquiring the same non-reentrant mutex again blocked forever. UI snapshot polling then blocked on that mutex too. The log call itself was never reached.
- Fix: read generation and submitted_samples under one short lock, then format and write the log after releasing it.
- Validation: interrupt_worker_replies_and_keeps_snapshot_readable failed on the original code; passed after the patch. The test exercises two consecutive interruptions, readable shared state, and worker shutdown on Windows without opening an audio device.
- Application acceptance: the release executable started on the affected host at 02:53:11 UTC. Audio interruption completed in 0 ms; frontend initial_ipc_ready completed at 02:53:12.761 UTC, 183 ms after script initialization. Windows reported Responding=True, the UI exposed its controls, and the RC003 completed ATVV capability negotiation.
- Privacy: no device identifiers, personal paths, audio, credentials, or third-party private data included.
