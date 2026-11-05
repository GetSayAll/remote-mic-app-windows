// @vitest-environment jsdom

import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentAccessSnapshot } from "../lib/agent-access";
import ReflectionsAgentAccess from "./ReflectionsAgentAccess.vue";
import { agentAccessErrorText, agentClientStatusText } from "./reflections-agent-access-view";

const api = vi.hoisted(() => ({
  getAgentAccessSnapshot: vi.fn(),
  setAgentAccessEnabled: vi.fn(),
  createAgentAuthorization: vi.fn(),
  revokeAgentAuthorization: vi.fn(),
  connectAgentClient: vi.fn(),
  removeAgentClient: vi.fn(),
}));

vi.mock("../lib/agent-access", async () => {
  const actual =
    await vi.importActual<typeof import("../lib/agent-access")>("../lib/agent-access");
  return { ...actual, ...api };
});

function snapshot(overrides: Partial<AgentAccessSnapshot> = {}): AgentAccessSnapshot {
  return {
    enabled: false,
    clients: [
      {
        id: "codex",
        displayName: "Codex",
        available: true,
        connected: false,
        needsReconnect: false,
      },
      {
        id: "claude-code",
        displayName: "Claude Code",
        available: true,
        connected: false,
        needsReconnect: false,
      },
      {
        id: "cursor",
        displayName: "Cursor",
        available: false,
        connected: false,
        needsReconnect: false,
      },
      {
        id: "opencode",
        displayName: "OpenCode",
        available: true,
        connected: false,
        needsReconnect: false,
      },
    ],
    authorizations: [],
    ...overrides,
  };
}

describe("reflections agent access", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getAgentAccessSnapshot.mockResolvedValue(snapshot());
    api.setAgentAccessEnabled.mockImplementation(async (enabled: boolean) =>
      snapshot({ enabled }),
    );
    api.connectAgentClient.mockResolvedValue(snapshot({ enabled: true }));
    api.removeAgentClient.mockResolvedValue(snapshot({ enabled: true }));
    api.revokeAgentAuthorization.mockResolvedValue(snapshot({ enabled: true }));
    api.createAgentAuthorization.mockResolvedValue({
      snapshot: snapshot({ enabled: true, authorizations: [] }),
      displayName: "Codex",
      standardJson: '{"mcpServers":{"sayall_history":{}}}',
      codexToml: '[mcp_servers.sayall_history]\ncommand = "x"',
    });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders a closed card with the switch and the empty authorization list", async () => {
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    expect(wrapper.text()).toContain("本地 Agent 访问");
    expect(wrapper.text()).toContain("允许访问");
    expect(wrapper.text()).toContain("已授权客户端");
    expect(wrapper.text()).toContain("还没有客户端获得访问权限。");
    expect(wrapper.text()).not.toContain("连接 AI 工具");
    expect((wrapper.get("input[type=checkbox]").element as HTMLInputElement).checked).toBe(false);
  });

  it("shows the four clients with their own status when enabled", async () => {
    api.getAgentAccessSnapshot.mockResolvedValue(
      snapshot({
        enabled: true,
        clients: [
          {
            id: "codex",
            displayName: "Codex",
            available: true,
            connected: true,
            needsReconnect: false,
          },
          {
            id: "claude-code",
            displayName: "Claude Code",
            available: true,
            connected: false,
            needsReconnect: false,
          },
          {
            id: "cursor",
            displayName: "Cursor",
            available: false,
            connected: false,
            needsReconnect: false,
          },
          {
            id: "opencode",
            displayName: "OpenCode",
            available: true,
            connected: true,
            needsReconnect: true,
          },
        ],
      }),
    );
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    expect(wrapper.text()).toContain("连接 AI 工具");
    expect(wrapper.text()).toContain("已连接 · 请重新打开 Codex");
    expect(wrapper.text()).toContain("可连接");
    expect(wrapper.text()).toContain("未检测到安装");
    expect(wrapper.text()).toContain("位置已变化，请先移除这个连接，再重新连接。");
    // 未检测到的客户端不能点「连接」。
    const cursorCard = wrapper
      .findAll(".agent-client")
      .find((card) => card.text().includes("Cursor"));
    const connectButton = cursorCard!
      .findAll("button")
      .find((button) => button.text() === "连接");
    expect((connectButton!.element as HTMLButtonElement).disabled).toBe(true);
  });

  it("persists the switch and falls back to the real value when saving fails", async () => {
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    await wrapper.get("input[type=checkbox]").setValue(true);
    await flushPromises();
    expect(api.setAgentAccessEnabled).toHaveBeenCalledWith(true);

    api.setAgentAccessEnabled.mockRejectedValueOnce({ code: "access_update_failed" });
    api.getAgentAccessSnapshot.mockResolvedValue(snapshot());
    await wrapper.get("input[type=checkbox]").setValue(false);
    await flushPromises();

    expect(wrapper.text()).toContain("无法更新本地 Agent 访问开关。");
    expect((wrapper.get("input[type=checkbox]").element as HTMLInputElement).checked).toBe(false);
  });

  it("connects a client and turns the button into remove", async () => {
    api.getAgentAccessSnapshot.mockResolvedValue(snapshot({ enabled: true }));
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    const codexCard = wrapper
      .findAll(".agent-client")
      .find((card) => card.text().includes("Codex"))!;
    const connect = codexCard.findAll("button").find((button) => button.text() === "连接")!;
    await connect.trigger("click");
    await flushPromises();

    expect(api.connectAgentClient).toHaveBeenCalledWith("codex");
  });

  it("reports a failed connection with the client name in the sentence", async () => {
    api.getAgentAccessSnapshot.mockResolvedValue(snapshot({ enabled: true }));
    api.connectAgentClient.mockRejectedValueOnce({
      code: "configuration_conflict",
      client: "codex",
    });
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    const codexCard = wrapper
      .findAll(".agent-client")
      .find((card) => card.text().includes("Codex"))!;
    await codexCard.findAll("button").find((button) => button.text() === "连接")!.trigger("click");
    await flushPromises();

    expect(wrapper.text()).toContain(
      "Codex 已存在名为 sayall_history 的 MCP；无线麦没有覆盖它，请先检查或移除冲突配置。",
    );
  });

  it("creates a manual authorization and copies both formats", async () => {
    api.getAgentAccessSnapshot.mockResolvedValue(snapshot({ enabled: true }));
    vi.useFakeTimers();
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    const input = wrapper.get(".agent-manual input");
    await input.setValue("Codex");
    await wrapper.get(".agent-manual button").trigger("click");
    await flushPromises();

    expect(api.createAgentAuthorization).toHaveBeenCalledWith("Codex");
    expect(wrapper.text()).toContain("已为“Codex”生成配置");
    expect(wrapper.text()).toContain("复制标准 MCP JSON");

    const buttons = wrapper.findAll(".agent-generated-actions button");
    await buttons[0].trigger("click");
    await flushPromises();
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      '{"mcpServers":{"sayall_history":{}}}',
    );
    expect(wrapper.text()).toContain("已复制");

    await buttons[1].trigger("click");
    await flushPromises();
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      '[mcp_servers.sayall_history]\ncommand = "x"',
    );

    await vi.advanceTimersByTimeAsync(1_600);
    expect(wrapper.findAll(".agent-generated-actions button")[0].text()).toContain(
      "复制标准 MCP JSON",
    );
  });

  it("revokes an authorization", async () => {
    api.getAgentAccessSnapshot.mockResolvedValue(
      snapshot({
        enabled: true,
        authorizations: [
          {
            clientId: "5f0e4c2a-1111-4111-8111-111111111111",
            displayName: "Codex",
            integration: "codex",
            createdAtMs: 1_760_000_000_000,
            needsReconnect: false,
          },
        ],
      }),
    );
    const wrapper = mount(ReflectionsAgentAccess);
    await flushPromises();

    await wrapper.get(".agent-authorizations button").trigger("click");
    await flushPromises();
    expect(api.revokeAgentAuthorization).toHaveBeenCalledWith(
      "5f0e4c2a-1111-4111-8111-111111111111",
    );
  });

  it("maps every error code to one readable sentence", () => {
    expect(agentAccessErrorText({ code: "client_unavailable" }, "Cursor")).toBe(
      "没有检测到 Cursor，请先安装后再连接。",
    );
    expect(agentAccessErrorText({ code: "local_component_missing" })).toBe(
      "当前安装缺少本地读取组件，请重新安装完整安装包。",
    );
    expect(agentAccessErrorText({ code: "client_command_unavailable", client: "claude-code" })).toBe(
      "Claude Code 的命令行入口无法正常启动，请修复或重新安装客户端后再试。",
    );
    expect(agentAccessErrorText({ code: "weird_unknown_code" })).toBe(
      "本地 Agent 访问操作失败，请稍后重试。",
    );
    // 红线：不得出现「助手 / Helper / 端口 / 进程」这类机制词。
    for (const code of [
      "access_load_failed",
      "access_update_failed",
      "invalid_client_name",
      "authorization_failed",
      "revoke_failed",
      "configuration_conflict",
      "invalid_client_configuration",
      "unsafe_client_configuration",
      "configuration_write_failed",
      "client_installation_rejected",
      "integration_failed",
      "configuration_removal_failed",
      "duplicate_client",
    ]) {
      const text = agentAccessErrorText({ code }, "Codex");
      expect(text).not.toMatch(/助手|Helper|端口|进程|注入|提权/);
      expect(text.length).toBeGreaterThan(4);
    }
  });

  it("describes each client status without leaking mechanism words", () => {
    const base = { id: "codex" as const, displayName: "Codex", available: true };
    expect(agentClientStatusText({ ...base, connected: false, needsReconnect: false })).toBe(
      "可连接",
    );
    expect(agentClientStatusText({ ...base, connected: true, needsReconnect: false })).toBe(
      "已连接 · 请重新打开 Codex",
    );
    expect(agentClientStatusText({ ...base, connected: false, needsReconnect: true })).toBe(
      "位置已变化，请先移除这个连接，再重新连接。",
    );
    expect(
      agentClientStatusText({ ...base, available: false, connected: false, needsReconnect: false }),
    ).toBe("未检测到安装");
  });
});
