<script setup lang="ts">
import type { RuntimeSnapshot } from "../lib/bridge";
defineProps<{ runtime: RuntimeSnapshot | null }>();
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>权限</h1>
        <p>主程序保持普通用户权限，只使用 Windows 公共 API。</p>
      </div>
    </header>

    <article class="card permission-list">
      <div class="permission-row">
        <div class="permission-icon">BT</div>
        <div><strong>Bluetooth LE</strong><p>读取已配对设备，并在后续阶段连接 RC003 GATT 服务。</p></div>
        <span class="badge" :class="runtime?.platform.bleScanAvailable ? 'warning' : 'pending'">
          {{ runtime?.platform.bleScanAvailable ? "API 可调用，待验证" : "当前主机不可用" }}
        </span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">IN</div>
        <div><strong>Raw Input 与 SendInput</strong><p>仅处理明确识别的遥控器，不拦截普通键盘。</p></div>
        <span class="badge pending">尚未实现</span>
      </div>
      <div class="permission-row">
        <div class="permission-icon">AU</div>
        <div><strong>音频端点</strong><p>用户明确选择 WASAPI 输出端点，不更改系统默认设备。</p></div>
        <span class="badge pending">尚未实现</span>
      </div>
    </article>

    <div class="info-callout">
      基础语音路径不依赖 Frida、管理员权限、第三方 App 私有配置或虚拟 HID 驱动。
    </div>
  </section>
</template>
