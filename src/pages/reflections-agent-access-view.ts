// 回眸「本地 Agent 访问」的文案判定（纯函数，便于逐条覆盖）。
//
// 文案与 Mac `statistics.transcripts.agent_access.*` 对齐；只有「本地 MCP Helper」
// 与产品名两处按 Windows 文案红线改写（见 docs/product-copy.md）。

import type { AgentAccessErrorPayload, AgentClientView } from "../lib/agent-access";
import { t } from "../lib/i18n";

/** 客户端卡片上的状态句。优先级与 Mac 一致：位置变化 > 已连接 > 可连接 > 未安装。 */
export function agentClientStatusText(client: AgentClientView): string {
  if (client.needsReconnect) {
    return t("位置已变化，请先移除这个连接，再重新连接。");
  }
  if (client.connected) {
    return t("已连接 · 请重新打开 {name}", { name: client.displayName });
  }
  if (client.available) {
    return t("可连接");
  }
  return t("未检测到安装");
}

const ERROR_KEYS: Record<string, string> = {
  access_disabled: "本地 Agent 访问已关闭。",
  access_load_failed: "无法读取本地 Agent 访问状态。",
  access_state_invalid: "无法读取本地 Agent 访问状态。",
  access_update_failed: "无法更新本地 Agent 访问开关。",
  invalid_client_name: "客户端名称需要包含 1–100 个字符。",
  authorization_failed: "无法创建客户端授权。",
  authorization_not_found: "这个授权已经不可用，请刷新页面后再试。",
  revoke_failed: "无法撤销这个客户端。",
  local_component_missing: "当前安装缺少本地读取组件，请重新安装完整安装包。",
  client_unavailable: "没有检测到 {name}，请先安装后再连接。",
  client_command_unavailable:
    "{name} 的命令行入口无法正常启动，请修复或重新安装客户端后再试。",
  configuration_conflict:
    "{name} 已存在名为 sayall_history 的 MCP；无线麦没有覆盖它，请先检查或移除冲突配置。",
  invalid_client_configuration:
    "{name} 的 MCP 配置不是合法 JSON；无线麦没有修改原文件，请修复配置后再试。",
  unsafe_client_configuration: "{name} 的配置路径不安全或是符号链接；无线麦拒绝写入。",
  configuration_write_failed:
    "无法安全写入 {name} 的 MCP 配置，请检查文件权限后再试。",
  client_configuration_write_failed:
    "无法安全写入 {name} 的 MCP 配置，请检查文件权限后再试。",
  client_installation_rejected: "{name} 拒绝了 MCP 安装命令，请检查客户端状态后再试。",
  integration_failed:
    "{name} 自动连接失败，本次临时授权没有保留。请修复客户端后重试或使用手动配置。",
  configuration_removal_failed:
    "{name} 的访问授权已撤销，但无法自动移除客户端配置。请手动删除 sayall_history。",
  duplicate_client: "这个客户端已经有一份有效授权，请先使用现有授权或撤销后再创建。",
  request_failed: "本地 Agent 访问操作失败，请稍后重试。",
};

/** 错误体 → 一句人话。`name` 取客户端显示名，服务端没给就用「这个客户端」。 */
export function agentAccessErrorText(
  payload: AgentAccessErrorPayload,
  clientName?: string | null,
): string {
  const source = ERROR_KEYS[payload.code] ?? ERROR_KEYS.request_failed;
  const name = clientName ?? payload.client ?? "这个客户端";
  const resolved = payload.client && !clientName ? clientDisplayName(payload.client) : name;
  return t(source, { name: resolved });
}

function clientDisplayName(client: string): string {
  switch (client) {
    case "codex":
      return "Codex";
    case "claude-code":
      return "Claude Code";
    case "cursor":
      return "Cursor";
    case "opencode":
      return "OpenCode";
    default:
      return "这个客户端";
  }
}
