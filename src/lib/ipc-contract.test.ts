import { describe, expect, it } from "vitest";
import contract from "../../contracts/ipc/windows-runtime.json";
import type {
  AudioPhase,
  ConnectionPhase,
  PairedRemote,
  PlatformSnapshot,
  RawInputPhase,
  RemoteButton,
  RemoteModel,
  VoiceSessionState,
} from "./bridge";

const connectionPhases = [
  "idle",
  "connecting",
  "discovering",
  "awaiting_capabilities",
  "ready",
  "streaming",
  "draining",
  "reconnecting",
  "suspended",
  "disconnected",
  "failed",
] as const satisfies readonly ConnectionPhase[];
const voiceStates = ["idle", "streaming", "draining"] as const satisfies readonly VoiceSessionState[];
const remoteModels = ["rc001", "rc003", "unknown"] as const satisfies readonly RemoteModel[];
const audioPhases = [
  "unconfigured",
  "ready",
  "streaming",
  "draining",
  "failed",
  "unsupported",
] as const satisfies readonly AudioPhase[];
const rawInputPhases = [
  "stopped",
  "starting",
  "ready",
  "failed",
  "unsupported",
] as const satisfies readonly RawInputPhase[];
const remoteButtons = [
  "back",
  "ok",
  "tv",
  "home",
  "right",
  "left",
  "down",
  "up",
  "menu",
  "power",
  "volume_mute",
  "volume_up",
  "volume_down",
] as const satisfies readonly RemoteButton[];

describe("Rust and TypeScript IPC contract", () => {
  it("loads the shared platform snapshot through the frontend types", () => {
    const fixture = contract.platformSnapshot;
    const platformSnapshot: PlatformSnapshot = {
      ...fixture,
      connection: {
        ...fixture.connection,
        phase: memberOf(fixture.connection.phase, connectionPhases),
        remoteModel: memberOf(fixture.connection.remoteModel, remoteModels),
        voiceState: memberOf(fixture.connection.voiceState, voiceStates),
      },
      audio: {
        ...fixture.audio,
        phase: memberOf(fixture.audio.phase, audioPhases),
      },
      rawInput: {
        ...fixture.rawInput,
        phase: memberOf(fixture.rawInput.phase, rawInputPhases),
        lastButton:
          fixture.rawInput.lastButton === null
            ? null
            : memberOf(fixture.rawInput.lastButton, remoteButtons),
      },
    };

    expect(platformSnapshot).toEqual(fixture);
    expectExactKeys(platformSnapshot, [
      "platform",
      "windowsApiAvailable",
      "bleScanAvailable",
      "bleVoiceReady",
      "wasapiReady",
      "rawInputReady",
      "sendInputReady",
      "verificationStatus",
      "connection",
      "audio",
      "rawInput",
    ]);
    expectExactKeys(platformSnapshot.connection, [
      "phase",
      "remoteName",
      "remoteModel",
      "capabilities",
      "voiceState",
      "decodedSamples",
      "generation",
      "reconnectAttempt",
      "powerNotificationsAvailable",
      "lastError",
    ]);
    expectExactKeys(platformSnapshot.connection.capabilities!, [
      "version",
      "codecs",
      "interaction",
      "frameSize",
      "selectedCodec",
      "sampleRate",
    ]);
    expectExactKeys(platformSnapshot.audio, [
      "phase",
      "selectedEndpointId",
      "selectedEndpointName",
      "queuedSamples",
      "submittedSamples",
      "generation",
      "lastError",
    ]);
    expectExactKeys(platformSnapshot.rawInput, [
      "phase",
      "matchedDeviceCount",
      "rawEventCount",
      "semanticEdgeCount",
      "lastButton",
      "lastIsPressed",
      "lastError",
    ]);
    expectNoSnakeCaseKeys(platformSnapshot);
    expect(platformSnapshot).not.toHaveProperty("connection.remote_model");
  });

  it("keeps RC001, RC003 and unknown paired-remote models stable", () => {
    const pairedRemotes: PairedRemote[] = contract.pairedRemotes.map((remote) => ({
      ...remote,
      model: memberOf(remote.model, remoteModels),
    }));

    expect(pairedRemotes.map((remote) => remote.model)).toEqual(["rc001", "rc003", "unknown"]);
    for (const remote of pairedRemotes) {
      expectExactKeys(remote, ["id", "name", "model", "isSupportedCandidate"]);
      expect(remote).not.toHaveProperty("is_supported_candidate");
    }
  });
});

function memberOf<T extends string>(value: string, allowed: readonly T[]): T {
  expect(allowed).toContain(value);
  return value as T;
}

function expectExactKeys(value: object, expectedKeys: string[]): void {
  expect(Object.keys(value).sort()).toEqual([...expectedKeys].sort());
}

function expectNoSnakeCaseKeys(value: unknown): void {
  if (Array.isArray(value)) {
    value.forEach(expectNoSnakeCaseKeys);
    return;
  }
  if (value === null || typeof value !== "object") {
    return;
  }
  for (const [key, child] of Object.entries(value)) {
    expect(key).not.toContain("_");
    expectNoSnakeCaseKeys(child);
  }
}
