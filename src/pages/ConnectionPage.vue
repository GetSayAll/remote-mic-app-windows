<script setup lang="ts">
import { ref } from "vue";
import type { PairedRemote, RuntimeSnapshot } from "../lib/bridge";
import { scanPairedRemotes } from "../lib/bridge";

defineProps<{ runtime: RuntimeSnapshot | null }>();

const scanning = ref(false);
const devices = ref<PairedRemote[]>([]);
const scanMessage = ref("尚未扫描");

async function scan() {
  scanning.value = true;
  scanMessage.value = "正在读取 Windows 已配对的 BLE 设备…";
  try {
    devices.value = await scanPairedRemotes();
    scanMessage.value = devices.value.length
      ? `找到 ${devices.value.length} 个已批准名称的候选设备`
      : "没有找到已配对且名称精确匹配的 RC003 候选设备";
  } catch (error) {
    devices.value = [];
    scanMessage.value = error instanceof Error ? error.message : String(error);
  } finally {
    scanning.value = false;
  }
}
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>连接与语音</h1>
        <p>只有完成 GATT 特征发现、通知订阅和 ATVV 能力确认后，设备才能进入语音就绪。</p>
      </div>
      <span class="badge pending">{{ runtime?.platform.verificationStatus ?? "正在读取运行状态" }}</span>
    </header>

    <div class="two-column">
      <article class="card">
        <div class="card-title-row">
          <div>
            <h2>RC003 蓝牙连接</h2>
            <p class="muted">只扫描 Windows 中已经配对、且名称在白名单内的 BLE 设备。</p>
          </div>
          <button class="primary-button" type="button" :disabled="scanning || !runtime?.platform.bleScanAvailable" @click="scan">
            {{ scanning ? "扫描中…" : "扫描已配对设备" }}
          </button>
        </div>
        <div class="status-panel">
          <span class="status-dot" :class="runtime?.platform.bleScanAvailable ? 'warning' : 'pending'"></span>
          <div><strong>{{ scanMessage }}</strong><small>不会仅凭发现设备名称显示“语音可用”</small></div>
        </div>
        <ul v-if="devices.length" class="device-list">
          <li v-for="device in devices" :key="device.id">
            <div><strong>{{ device.name }}</strong><small>已配对候选设备</small></div>
            <button type="button" disabled>连接（下一阶段）</button>
          </li>
        </ul>
      </article>

      <article class="card">
        <h2>语音输出</h2>
        <div class="setting-list compact">
          <div class="setting-row"><strong>ATVV / 16 kHz</strong><span>纯 Rust 协议测试已建立</span></div>
          <div class="setting-row"><strong>WASAPI 端点</strong><span>等待 Windows 实现与验证</span></div>
          <div class="setting-row"><strong>增益</strong><span>0 dB</span></div>
        </div>
        <div class="info-callout warning">
          当前版本不会修改系统默认输入或输出，也不会静默安装 VB-CABLE。
        </div>
      </article>
    </div>
  </section>
</template>
