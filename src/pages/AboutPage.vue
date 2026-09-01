<script setup lang="ts">
import type { RuntimeSnapshot } from "../lib/bridge";
defineProps<{ runtime: RuntimeSnapshot | null }>();
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>关于</h1>
        <p>把 RC003 的按键和麦克风连接到 Windows。</p>
      </div>
    </header>

    <article class="card about-card">
      <div class="app-icon">S</div>
      <div>
        <h2>无线麦 SayAll Windows 版</h2>
        <p>版本 {{ runtime?.appVersion ?? "0.1.0" }} · Rust + Tauri 2 + Vue 3</p>
        <p class="muted">GPL-3.0-only · 独立公开仓库，不继承参考项目的 Git 历史。</p>
      </div>
    </article>

    <article class="card">
      <h2>当前边界</h2>
      <div class="setting-list compact">
        <div class="setting-row"><strong>产品设计基准</strong><span>无线麦 macOS 原版</span></div>
        <div class="setting-row"><strong>支持设备</strong><span>RC003（待 Windows 真机验收）</span></div>
        <div class="setting-row"><strong>运行平台</strong><span>{{ runtime?.platform.platform ?? "读取中" }}</span></div>
        <div class="setting-row"><strong>状态</strong><span>{{ runtime?.platform.verificationStatus ?? "读取中" }}</span></div>
      </div>
    </article>
  </section>
</template>
