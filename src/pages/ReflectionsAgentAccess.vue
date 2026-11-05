<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  agentAccessErrorPayload,
  connectAgentClient,
  createAgentAuthorization,
  getAgentAccessSnapshot,
  removeAgentClient,
  revokeAgentAuthorization,
  setAgentAccessEnabled,
  type AgentAccessSnapshot,
  type AgentClientId,
  type AgentClientView,
  type CreatedAgentAuthorization,
} from "../lib/agent-access";
import { t } from "../lib/i18n";
import { agentAccessErrorText, agentClientStatusText } from "./reflections-agent-access-view";

/**
 * 回眸页底部「本地 Agent 访问」（对齐 Mac `TranscriptAgentAccessSection`）。
 *
 * 状态全部来自 Rust 侧快照，前端不缓存授权：切页、重开都会重新读，
 * 这样「关掉开关 / 撤销」在界面与 MCP 侧永远一致。
 */
const snapshot = ref<AgentAccessSnapshot | null>(null);
const loading = ref(true);
const toggling = ref(false);
const working = ref<AgentClientId | null>(null);
const clientName = ref("");
const creating = ref(false);
const generated = ref<CreatedAgentAuthorization | null>(null);
const copied = ref<"json" | "toml" | null>(null);
const errorText = ref("");

const enabled = computed(() => snapshot.value?.enabled ?? false);
const clients = computed<AgentClientView[]>(() => snapshot.value?.clients ?? []);
const authorizations = computed(() => snapshot.value?.authorizations ?? []);
const canCreate = computed(() => clientName.value.trim().length > 0 && !creating.value);

async function refresh(): Promise<void> {
  try {
    snapshot.value = await getAgentAccessSnapshot();
  } catch (error) {
    errorText.value = agentAccessErrorText(agentAccessErrorPayload(error));
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);

function applyError(error: unknown, clientNameForCopy?: string): void {
  errorText.value = agentAccessErrorText(
    agentAccessErrorPayload(error),
    clientNameForCopy,
  );
}

async function toggleEnabled(event: Event): Promise<void> {
  const target = event.target as HTMLInputElement;
  const next = target.checked;
  toggling.value = true;
  errorText.value = "";
  try {
    snapshot.value = await setAgentAccessEnabled(next);
  } catch (error) {
    // 回读真值：开关不能停在用户没成功保存的位置上。
    await refresh();
    applyError(error);
  } finally {
    toggling.value = false;
  }
}

async function connect(client: AgentClientView): Promise<void> {
  if (working.value) return;
  working.value = client.id;
  errorText.value = "";
  generated.value = null;
  try {
    snapshot.value = await connectAgentClient(client.id);
  } catch (error) {
    applyError(error, client.displayName);
  } finally {
    working.value = null;
  }
}

async function remove(client: AgentClientView): Promise<void> {
  if (working.value) return;
  working.value = client.id;
  errorText.value = "";
  try {
    snapshot.value = await removeAgentClient(client.id);
  } catch (error) {
    // 「授权已撤销但配置没删掉」也算失败：必须让用户看见并知道要手动删。
    await refresh();
    applyError(error, client.displayName);
  } finally {
    working.value = null;
  }
}

async function create(): Promise<void> {
  if (!canCreate.value) return;
  creating.value = true;
  errorText.value = "";
  copied.value = null;
  try {
    generated.value = await createAgentAuthorization(clientName.value.trim());
    snapshot.value = generated.value.snapshot;
    clientName.value = "";
  } catch (error) {
    applyError(error);
  } finally {
    creating.value = false;
  }
}

async function revoke(clientId: string): Promise<void> {
  errorText.value = "";
  try {
    snapshot.value = await revokeAgentAuthorization(clientId);
    if (generated.value) {
      generated.value = null;
    }
  } catch (error) {
    applyError(error);
  }
}

async function copy(kind: "json" | "toml"): Promise<void> {
  if (!generated.value) return;
  const text =
    kind === "json" ? generated.value.standardJson : generated.value.codexToml;
  try {
    await navigator.clipboard.writeText(text);
    copied.value = kind;
    window.setTimeout(() => {
      if (copied.value === kind) copied.value = null;
    }, 1_500);
  } catch {
    errorText.value = t("复制失败，请检查剪贴板权限后重试。");
  }
}

function formatCreatedAt(millis: number): string {
  return new Date(millis).toLocaleString();
}
</script>

<template>
  <section class="card agent-access">
    <header class="agent-access-header">
      <div class="agent-access-copy">
        <h3>{{ t("本地 Agent 访问") }}</h3>
        <p>{{ t("允许你授权的 Agent 通过本机 MCP 只读访问回眸记录。") }}</p>
      </div>
      <label class="toggle-row">
        <span>{{ t("允许访问") }}</span>
        <input
          class="toggle-input"
          type="checkbox"
          :checked="enabled"
          :disabled="toggling || loading"
          @change="toggleEnabled"
        />
      </label>
    </header>

    <p class="agent-access-privacy">
      {{ t("无线麦不会上传记录；被授权的 AI 客户端可能把读取的文字发送给自己的服务商。开关默认关闭，关闭后所有客户端立即无法读取。") }}
    </p>

    <template v-if="enabled">
      <div class="agent-access-divider"></div>

      <div class="agent-access-section">
        <h4>{{ t("连接 AI 工具") }}</h4>
        <p>
          {{ t("无线麦会为所选工具创建独立只读授权并写入本机配置；连接后请关闭并重新打开对应 AI 工具，无需重启无线麦。") }}
        </p>
        <div class="agent-clients">
          <div v-for="client in clients" :key="client.id" class="agent-client">
            <span class="agent-client-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none">
                <rect x="6.5" y="6.5" width="11" height="11" rx="2.6" stroke="currentColor" stroke-width="1.6"/>
                <path d="M9.8 3.4v3.1M14.2 3.4v3.1M9.8 17.5v3.1M14.2 17.5v3.1M3.4 9.8h3.1M3.4 14.2h3.1M17.5 9.8h3.1M17.5 14.2h3.1" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
              </svg>
            </span>
            <div class="agent-client-meta">
              <strong>{{ client.displayName }}</strong>
              <span
                class="agent-client-status"
                :class="{ connected: client.connected, warning: client.needsReconnect }"
              >{{ agentClientStatusText(client) }}</span>
            </div>
            <span v-if="working === client.id" class="agent-spinner" aria-hidden="true"></span>
            <button
              v-else-if="client.connected"
              class="secondary-button"
              type="button"
              :disabled="working !== null"
              @click="remove(client)"
            >
              {{ t("移除") }}
            </button>
            <button
              v-else
              class="primary-button"
              type="button"
              :disabled="!client.available || working !== null"
              @click="connect(client)"
            >
              {{ t("连接") }}
            </button>
          </div>
        </div>
      </div>

      <div class="agent-access-divider"></div>

      <div class="agent-access-section">
        <h4>{{ t("其他客户端或手动配置") }}</h4>
        <p>
          {{ t("自动连接不可用时，创建一个授权并复制对应格式。配置中的访问令牌只会显示一次。") }}
        </p>
        <div class="agent-manual">
          <input
            v-model="clientName"
            type="text"
            maxlength="100"
            :placeholder="t('例如 Codex 或 Claude Desktop')"
            :aria-label="t('客户端名称')"
            @keyup.enter="create"
          />
          <button
            class="primary-button"
            type="button"
            :disabled="!canCreate"
            @click="create"
          >
            {{ t("创建只读授权") }}
          </button>
        </div>

        <div v-if="generated" class="agent-generated">
          <p class="agent-generated-title">
            {{ t("已为“{name}”生成配置", { name: generated.displayName }) }}
          </p>
          <p>
            {{ t("访问令牌只在这次生成的配置中提供。复制后请立即粘贴到对应客户端，不要保存到公开文件或聊天中。") }}
          </p>
          <div class="agent-generated-actions">
            <button class="secondary-button" type="button" @click="copy('json')">
              {{ copied === "json" ? t("已复制") : t("复制标准 MCP JSON") }}
            </button>
            <button class="secondary-button" type="button" @click="copy('toml')">
              {{ copied === "toml" ? t("已复制") : t("复制 Codex TOML") }}
            </button>
          </div>
        </div>
      </div>

      <div class="agent-access-divider"></div>
    </template>

    <div class="agent-access-section">
      <h4>{{ t("已授权客户端") }}</h4>
      <p v-if="authorizations.length === 0">{{ t("还没有客户端获得访问权限。") }}</p>
      <ul v-else class="agent-authorizations">
        <li v-for="authorization in authorizations" :key="authorization.clientId">
          <span class="agent-client-icon" aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none">
              <path d="M5 19V9.5M5 9.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Zm0 0h14M19 19V9.5m0 0a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </span>
          <span class="agent-authorization-meta">
            <strong>{{ authorization.displayName }}</strong>
            <span>{{ formatCreatedAt(authorization.createdAtMs) }}</span>
            <span v-if="authorization.needsReconnect" class="agent-client-status warning">
              {{ t("位置已变化，请先移除这个连接，再重新连接。") }}
            </span>
          </span>
          <button
            class="link-button danger"
            type="button"
            @click="revoke(authorization.clientId)"
          >
            {{ t("撤销") }}
          </button>
        </li>
      </ul>
    </div>

    <p v-if="errorText" class="agent-access-error" role="status">{{ errorText }}</p>
  </section>
</template>

<style scoped>
.agent-access {
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.agent-access-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}
.agent-access-copy h3 {
  margin: 0 0 4px;
  font-size: 15px;
  font-weight: 600;
}
.agent-access-copy p,
.agent-access-privacy,
.agent-access-section p {
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
}
.agent-access-divider {
  height: 1px;
  background: var(--border);
}
.agent-access-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.agent-access-section h4 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}
.agent-clients {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.agent-client {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  border-radius: 10px;
  background: var(--surface-subtle);
}
.agent-client-icon {
  display: inline-flex;
  width: 30px;
  height: 30px;
  flex: none;
  color: var(--text-secondary);
}
.agent-client-icon svg {
  width: 100%;
  height: 100%;
}
.agent-client-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.agent-client-meta strong {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.agent-client-status {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.4;
}
.agent-client-status.connected {
  color: var(--success-text);
}
.agent-client-status.warning {
  color: var(--warning-text);
}
.agent-spinner {
  width: 14px;
  height: 14px;
  flex: none;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: agent-spin 0.7s linear infinite;
}
@keyframes agent-spin {
  to {
    transform: rotate(360deg);
  }
}
.agent-manual {
  display: flex;
  gap: 10px;
}
.agent-manual input {
  flex: 1;
  min-width: 0;
  padding: 8px 10px;
  font-size: 13px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface-control);
  color: var(--text-primary);
}
.agent-generated {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  border-radius: 10px;
  background: var(--accent-surface);
}
.agent-generated-title {
  font-size: 13px !important;
  font-weight: 600;
  color: var(--text-primary) !important;
}
.agent-generated-actions {
  display: flex;
  gap: 10px;
}
.agent-authorizations {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.agent-authorizations li {
  display: flex;
  align-items: center;
  gap: 10px;
}
.agent-authorization-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-secondary);
}
.agent-authorization-meta strong {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}
.agent-access-error {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--warning-text);
}
</style>
