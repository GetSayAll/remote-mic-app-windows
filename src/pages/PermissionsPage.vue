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
  if (props.runtime?.platform.bleScanAvailable) return { tone: "warning", label: "待验证" };
  return { tone: "pending", label: "当前电脑不支持" };
});

const inputStatus = computed(() => {
  if (props.runtime?.platform.rawInputReady) return { tone: "success", label: "已运行" };
  if (props.runtime?.platform.windowsApiAvailable) return { tone: "warning", label: "待验证" };
  return { tone: "pending", label: "当前电脑不支持" };
});

const audioStatus = computed(() => {
  if (props.runtime?.platform.wasapiReady) return { tone: "success", label: "已就绪" };
  if (props.runtime?.platform.windowsApiAvailable) return { tone: "warning", label: "待选择设备" };
  return { tone: "pending", label: "当前电脑不支持" };
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
      </div>
    </header>

    <article class="card permission-list">
      <div class="permission-row">
        <div class="permission-icon">BT</div>
        <div><strong>蓝牙</strong><p>读取已配对的遥控器并建立连接。</p></div>
        <span class="badge" :class="bluetoothStatus.tone">
          {{ bluetoothStatus.label }}
        </span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">IN</div>
        <div><strong>按键监听与模拟</strong><p>只监听小米遥控器的按键；按键模拟仅在自定义映射启用时使用。</p></div>
        <span class="badge" :class="inputStatus.tone">{{ inputStatus.label }}</span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">AU</div>
        <div><strong>音频设备</strong><p>语音设备由你明确选择，不改动系统默认设备。</p></div>
        <span class="badge" :class="audioStatus.tone">{{ audioStatus.label }}</span>
      </div>
    </article>

    <article class="card diagnostics-card">
      <div class="card-title-row">
        <div>
          <h2>诊断摘要</h2>
          <p class="muted">摘要不含设备地址、语音内容等隐私信息，可放心复制发给开发者排查问题。</p>
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
