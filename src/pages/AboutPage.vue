<script setup lang="ts">
import type { RuntimeSnapshot } from "../lib/bridge";
defineProps<{ runtime: RuntimeSnapshot | null }>();
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>关于</h1>
      </div>
    </header>

    <article class="card about-card">
      <img class="app-logo" src="/app-logo.png" alt="无线麦 SayAll 应用图标" />
      <div>
        <h2>无线麦 SayAll</h2>
        <p>版本 {{ runtime?.appVersion ?? "0.1.0" }}</p>
        <p class="muted">开源许可 GPL-3.0。</p>
      </div>
    </article>

    <article class="card">
      <h2>当前边界</h2>
      <div class="setting-list compact">
        <div class="setting-row"><strong>产品设计基准</strong><span>无线麦原版</span></div>
        <div class="setting-row"><strong>支持设备</strong><span>RC001 / RC003（待真机验收）</span></div>
        <div class="setting-row"><strong>状态</strong><span>{{ runtime?.platform.verificationStatus ?? "读取中" }}</span></div>
      </div>
    </article>
  </section>
</template>
