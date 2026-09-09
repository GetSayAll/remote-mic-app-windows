# WeType history can interrupt a continuously held voice key

- Discovered: 2026-09-09.
- Status: fixed in source; RC003 and WeType end-to-end acceptance pending.
- Scope: Windows v0.2.2 and v0.2.3, RC003 with WeType.
- Symptom: dictation starts, stops after roughly one second, then restarts and stops repeatedly while the remote voice key remains held.
- Trigger: multiple historical WeType executable entries in Windows microphone ConsentStore.
- Expected: one chord DOWN/UP pair for one physical voice hold, without restarting an already active recording.
- Evidence: the affected host has seven WeType history entries. The original enumeration returns the first, version 2.1.0.36 last used in July; the running version is 2.1.3.18 and the seventh entry updates in September. The original 700 ms detector reads the old entry, invokes TSF profile cycling, and schedules release/repress after 2, 3, and 5 seconds. Historical v0.2.2 session logs were unavailable, so the exact prior on-screen sequence remains user-reported.
- Cause: first-match registry enumeration is not current-version selection. The detector also captured its baseline after injection and treated an unavailable observation as failure. A queued stale retry removed held_hotkey before checking its session epoch.
- Fix: aggregate all matching entries, recognize active microphone use via LastUsedTimeStop, capture the baseline before injection, and suppress recovery for unknown observations. Recheck after the recovery delay and before releasing a chord. Validate session identity before consuming the held chord, preserving cleanup ownership if release fails.
- Timing: existing 700 ms checks, retry delays, chord spacing, and ATVV extension intervals are unchanged.
- Validation: five regression cases cover version history/order, an already active baseline, newly completed recording, genuinely unchanged inactive history, and unavailable/regressed observations. A separate ignored Windows test reads only aggregate observation availability and counts.
- Privacy: registry reads use the public Windows microphone access history. No WeType private configuration is read or written; diagnostics contain no paths, raw timestamps, voice data, or input text.
