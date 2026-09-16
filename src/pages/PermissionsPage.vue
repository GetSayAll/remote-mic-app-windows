<script setup lang="ts">
import { computed } from "vue";
import type { RuntimeSnapshot } from "../lib/bridge";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

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
  </section>
</template>
