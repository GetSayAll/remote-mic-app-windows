<script setup lang="ts">
const mappings = [
  ["方向键", "等待 Raw Input 真机采集"],
  ["OK / 首页 / 菜单", "等待 Windows 键值确认"],
  ["电视 / 电源", "实验能力，不进入首版承诺"],
  ["返回 / 音量", "Windows 公共 API 可用性待验证"],
];
</script>

<template>
  <section>
    <header class="page-header">
      <div>
        <h1>按键</h1>
        <p>按键映射将以真实 Windows Raw Input 报告为准，不继承未经复验的 13 键结论。</p>
      </div>
      <span class="badge pending">等待真机验证</span>
    </header>

    <div class="two-column remote-layout">
      <article class="card remote-card">
        <div class="remote-photo-wrap">
          <img src="/RC003-remote-photo.png" alt="小米蓝牙遥控器 2 Pro" />
        </div>
        <div>
          <h2>小米蓝牙遥控器 2 Pro</h2>
          <p class="muted">型号 RC003 · 语音键始终是按下开始、释放结束</p>
        </div>
      </article>

      <article class="card">
        <div class="card-title-row">
          <h2>映射状态</h2>
          <button class="secondary-button" type="button" disabled>编辑映射</button>
        </div>
        <div class="setting-list">
          <div v-for="mapping in mappings" :key="mapping[0]" class="setting-row">
            <strong>{{ mapping[0] }}</strong>
            <span>{{ mapping[1] }}</span>
          </div>
        </div>
        <div class="info-callout">
          语音键不会加入双击等待或长按阈值，避免增加首个响应延迟。
        </div>
      </article>
    </div>
  </section>
</template>
