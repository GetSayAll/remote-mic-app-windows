<script setup lang="ts">
import { computed } from "vue";
import type { RuntimeSnapshot } from "../../lib/bridge";
import { t } from "../../lib/i18n";

const props = defineProps<{ runtime: RuntimeSnapshot | null }>();

/**
 * 权限三项（2026-10-31 从侧栏「权限」页并入设置页，对齐 Mac
 * `inlinePermissionsSection`）：只显示本机真实状态，不提供虚构的授权操作。
 */
const bluetoothStatus = computed(() => {
  if (props.runtime?.platform.bleVoiceReady) return { tone: "success", label: t("语音链路已就绪") };
  if (props.runtime?.platform.bleScanAvailable) return { tone: "warning", label: t("待验证") };
  return { tone: "pending", label: t("当前电脑不支持") };
});

const inputStatus = computed(() => {
  if (props.runtime?.platform.rawInputReady) return { tone: "success", label: t("已运行") };
  if (props.runtime?.platform.windowsApiAvailable) return { tone: "warning", label: t("待验证") };
  return { tone: "pending", label: t("当前电脑不支持") };
});

const audioStatus = computed(() => {
  if (props.runtime?.platform.wasapiReady) return { tone: "success", label: t("已就绪") };
  if (props.runtime?.platform.windowsApiAvailable) return { tone: "warning", label: t("待选择设备") };
  return { tone: "pending", label: t("当前电脑不支持") };
});
</script>

<template>
  <section class="settings-section">
    <h2 class="section-title">{{ t("权限") }}</h2>
    <article class="card permission-list">
      <div class="permission-row">
        <div class="permission-icon">BT</div>
        <div><strong>{{ t("蓝牙") }}</strong><p>{{ t("读取已配对的遥控器并建立连接。") }}</p></div>
        <span class="badge" :class="bluetoothStatus.tone">
          {{ bluetoothStatus.label }}
        </span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">IN</div>
        <div><strong>{{ t("按键监听与映射") }}</strong><p>{{ t("只监听小米遥控器的按键；只有你为按键配置了动作时，才会向电脑发送按键。") }}</p></div>
        <span class="badge" :class="inputStatus.tone">{{ inputStatus.label }}</span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">AU</div>
        <div><strong>{{ t("音频设备") }}</strong><p>{{ t("语音设备由你明确选择，不改动系统默认设备。") }}</p></div>
        <span class="badge" :class="audioStatus.tone">{{ audioStatus.label }}</span>
      </div>
    </article>
  </section>
</template>
