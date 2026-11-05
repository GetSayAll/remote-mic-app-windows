// 回眸「本地 Agent 访问」的 IPC 包装与视图辅助函数。

import { invoke } from "@tauri-apps/api/core";

/** 回眸「本地 Agent 访问」支持的 AI 客户端（与 Rust 侧 `AgentClientId` 同值）。 */
export type AgentClientId = "codex" | "claude-code" | "cursor" | "opencode";

export interface AgentClientView {
  id: AgentClientId;
  displayName: string;
  available: boolean;
  connected: boolean;
  needsReconnect: boolean;
  /** 已安装客户端的真实图标（PNG data URL）；取不到用各自的中性图形兜底。 */
  iconDataUrl?: string | null;
}

export interface AgentAuthorizationView {
  clientId: string;
  displayName: string;
  integration: AgentClientId | null;
  createdAtMs: number;
  needsReconnect: boolean;
}

export interface AgentAccessSnapshot {
  enabled: boolean;
  clients: AgentClientView[];
  authorizations: AgentAuthorizationView[];
}

/** 手动创建授权的返回值：配置里含一次性令牌，只在这次显示。 */
export interface CreatedAgentAuthorization {
  snapshot: AgentAccessSnapshot;
  displayName: string;
  standardJson: string;
  codexToml: string;
}

/** Rust 侧错误体：`code` 决定文案，`client` 决定句子里填哪个名字。 */
export interface AgentAccessErrorPayload {
  code: string;
  client?: AgentClientId | null;
}

/** 把 Tauri 抛出来的东西归一成错误体；认不出的按通用失败处理。 */
export function agentAccessErrorPayload(error: unknown): AgentAccessErrorPayload {
  if (typeof error === "string") {
    return { code: error };
  }
  if (error && typeof error === "object") {
    const candidate = error as { code?: unknown; client?: unknown };
    if (typeof candidate.code === "string") {
      return {
        code: candidate.code,
        client:
          typeof candidate.client === "string"
            ? (candidate.client as AgentClientId)
            : null,
      };
    }
  }
  return { code: "request_failed" };
}

export async function getAgentAccessSnapshot(): Promise<AgentAccessSnapshot> {
  return invoke<AgentAccessSnapshot>("get_agent_access_snapshot");
}

export async function setAgentAccessEnabled(enabled: boolean): Promise<AgentAccessSnapshot> {
  return invoke<AgentAccessSnapshot>("set_agent_access_enabled", { enabled });
}

export async function createAgentAuthorization(
  displayName: string,
): Promise<CreatedAgentAuthorization> {
  return invoke<CreatedAgentAuthorization>("create_agent_authorization", { displayName });
}

export async function revokeAgentAuthorization(clientId: string): Promise<AgentAccessSnapshot> {
  return invoke<AgentAccessSnapshot>("revoke_agent_authorization", { clientId });
}

export async function connectAgentClient(client: AgentClientId): Promise<AgentAccessSnapshot> {
  return invoke<AgentAccessSnapshot>("connect_agent_client", { client });
}

export async function removeAgentClient(client: AgentClientId): Promise<AgentAccessSnapshot> {
  return invoke<AgentAccessSnapshot>("remove_agent_client", { client });
}
