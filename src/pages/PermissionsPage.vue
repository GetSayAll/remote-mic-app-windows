<script setup lang="ts">
import { computed, ref } from "vue";
import {
  formatDiagnosticReport,
  getDiagnosticReport,
  type RuntimeSnapshot,
} from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

const diagnosticText = ref("");
const diagnosticMessage = ref("尚未生成诊断摘要");
const generatingDiagnostic = ref(false);

const bluetoothStatus = computed(() => {
  if (props.runtime?.platform.bleVoiceReady) return { tone: "success", label: "语音链路已就绪" };
  if (props.runtime?.platform.bleScanAvailable) return { tone: "warning", label: "API 可用，待真机验证" };
  return { tone: "pending", label: "当前主机不可用" };
});

const inputStatus = computed(() => {
  if (props.runtime?.platform.rawInputReady) return { tone: "success", label: "Raw Input 已运行" };
  if (props.runtime?.platform.windowsApiAvailable) return { tone: "warning", label: "可启动，待真机验证" };
  return { tone: "pending", label: "当前主机不可用" };
});

const audioStatus = computed(() => {
  if (props.runtime?.platform.wasapiReady) return { tone: "success", label: "WASAPI 已就绪" };
  if (props.runtime?.platform.windowsApiAvailable) return { tone: "warning", label: "待选择输出端点" };
  return { tone: "pending", label: "当前主机不可用" };
});

async function generateDiagnostic() {
  generatingDiagnostic.value = true;
  diagnosticMessage.value = "正在读取当前运行状态…";
  try {
    diagnosticText.value = formatDiagnosticReport(await getDiagnosticReport());
    diagnosticMessage.value = "诊断摘要已生成；复制前可在页面内检查全部内容";
  } catch (error) {
    diagnosticText.value = "";
    diagnosticMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    generatingDiagnostic.value = false;
  }
}

async function copyDiagnostic() {
  if (!diagnosticText.value) await generateDiagnostic();
  if (!diagnosticText.value) return;
  try {
    if (!navigator.clipboard?.writeText) throw new Error("当前环境不支持剪贴板写入");
    await navigator.clipboard.writeText(diagnosticText.value);
    diagnosticMessage.value = "诊断摘要已复制到剪贴板";
  } catch (error) {
    diagnosticMessage.value = error instanceof Error ? error.message : String(error);
  }
}
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>权限</h1>
        <p>主程序保持普通用户权限，只使用 Windows 公共 API；状态只代表当前运行时，不替代真机验收。</p>
      </div>
    </header>

    <article class="card permission-list">
      <div class="permission-row">
        <div class="permission-icon">BT</div>
        <div><strong>Bluetooth LE</strong><p>读取已配对设备，并在后续阶段连接 RC003 GATT 服务。</p></div>
        <span class="badge" :class="bluetoothStatus.tone">
          {{ bluetoothStatus.label }}
        </span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">IN</div>
        <div><strong>Raw Input 与 SendInput</strong><p>只匹配 RC003 设备路径；自动映射仍等待真机事件形态确认。</p></div>
        <span class="badge" :class="inputStatus.tone">{{ inputStatus.label }}</span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">AU</div>
        <div><strong>音频端点</strong><p>用户明确选择 WASAPI 输出端点，不更改系统默认设备。</p></div>
        <span class="badge" :class="audioStatus.tone">{{ audioStatus.label }}</span>
      </div>
    </article>

    <div class="info-callout">
      基础语音路径不依赖 Frida、管理员权限、第三方 App 私有配置或虚拟 HID 驱动。
    </div>

    <article class="card diagnostics-card">
      <div class="card-title-row">
        <div>
          <h2>诊断摘要</h2>
          <p class="muted">只包含阶段、能力、代次和计数；不包含设备 ID、蓝牙地址、HID 路径、音频端点名称、错误原文、窗口标题、语音或用户文本。</p>
        </div>
        <div class="button-row">
          <button class="secondary-button" type="button" :disabled="generatingDiagnostic" @click="generateDiagnostic">
            {{ generatingDiagnostic ? "生成中…" : "生成摘要" }}
          </button>
          <button class="primary-button" type="button" :disabled="generatingDiagnostic" @click="copyDiagnostic">
            复制摘要
          </button>
        </div>
      </div>
      <p class="operation-message" aria-live="polite">{{ diagnosticMessage }}</p>
      <pre v-if="diagnosticText" class="diagnostic-output">{{ diagnosticText }}</pre>
    </article>
  </section>
</template>
