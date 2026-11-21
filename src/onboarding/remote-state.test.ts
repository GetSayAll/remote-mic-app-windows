/**
 * 连接/按键卡点快照：字段齐全、token 合法（Rust `diagnostic_token` 上限 64 字节）。
 */
import { describe, expect, it } from "vitest";
import { connectionPhaseCode, rawInputPhaseCode, remoteStateToken } from "./remote-state";

const worstHeartbeat = {
  snapshotAvailable: true,
  connectionPhase: "awaiting_capabilities" as const,
  bleVoiceReady: true,
  rawInputPhase: "unsupported" as const,
  matchedDeviceCount: 99,
  rawEventCount: 99_999,
  semanticEdgeCount: 99_999,
  staleRemoteEventCount: 99_999,
  reconnectAttempt: 999,
  deviceCount: 99,
  buttonObserved: false,
  connectionErrorPresent: false,
  rawInputErrorPresent: true,
};

describe("phase codes", () => {
  it("maps long connection/raw phases to short stable codes", () => {
    expect(connectionPhaseCode("awaiting_capabilities")).toBe("ac");
    expect(connectionPhaseCode("reconnecting")).toBe("rc");
    expect(rawInputPhaseCode("awaiting")).toBe("a");
    expect(rawInputPhaseCode("starting")).toBe("st");
  });
});

describe("remoteStateToken", () => {
  it("carries the fields needed to tell connection vs button-channel failures apart", () => {
    const token = remoteStateToken(
      {
        snapshotAvailable: true,
        connectionPhase: "ready",
        bleVoiceReady: true,
        rawInputPhase: "ready",
        matchedDeviceCount: 1,
        rawEventCount: 42,
        semanticEdgeCount: 8,
        staleRemoteEventCount: 0,
        reconnectAttempt: 0,
        deviceCount: 1,
        buttonObserved: true,
        connectionErrorPresent: false,
        rawInputErrorPresent: false,
      },
      {},
    );
    expect(token).toBe("p_r_ble_1_raw_r_m_1_re_42_se_8_st_0_rc_0_n_1_obs_1_err_0");
  });

  it("does not turn a missing runtime snapshot into a fake idle/stopped state", () => {
    expect(remoteStateToken({ ...worstHeartbeat, snapshotAvailable: false })).toBe("snap_0");
  });

  it("keeps the heartbeat token within the 64-byte diagnostic limit", () => {
    const token = remoteStateToken(worstHeartbeat);
    expect(token.length).toBeLessThanOrEqual(64);
    // 超长计数按上限截断，而不是生成 invalid token。
    expect(token).toContain("_re_999_");
    expect(token).toContain("_se_999_");
    expect(token).toContain("_rc_99_");
    expect(token).not.toContain("99999");
  });

  it("keeps state-transition tokens (with model, without counters) within 64 bytes", () => {
    const token = remoteStateToken(
      { ...worstHeartbeat, remoteModel: "unknown" },
      { includeCounters: false, includeModel: true, includeRawInputError: true },
    );
    expect(token).toBe("p_ac_ble_1_raw_u_m_9_rc_99_n_9_obs_0_err_0_ce_1_md_unknown");
    expect(token.length).toBeLessThanOrEqual(64);
  });

  it("fails visibly instead of silently logging Rust `invalid` when a caller overpacks the token", () => {
    expect(
      remoteStateToken(
        { ...worstHeartbeat, remoteModel: "unknown" },
        { includeCounters: true, includeModel: true, includeRawInputError: true },
      ),
    ).toBe("token_too_long");
  });

  it("can omit the device list and observed flag for the controls step", () => {
    const token = remoteStateToken(worstHeartbeat, {
      includeDeviceCount: false,
      includeButtonObserved: false,
    });
    expect(token).not.toContain("_n_");
    expect(token).not.toContain("_obs_");
  });
});
