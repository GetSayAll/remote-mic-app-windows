<template>
  <section>
    <header class="page-header">
      <div>
        <h1>统计</h1>
        <p>只保存每日汇总，不保存语音内容、识别文字、窗口标题或个人文件路径。</p>
      </div>
    </header>

    <div class="metric-grid">
      <article class="card metric-card"><span>今日按键</span><strong>0</strong><small>次</small></article>
      <article class="card metric-card"><span>今日语音</span><strong>0</strong><small>次</small></article>
      <article class="card metric-card"><span>语音时长</span><strong>0</strong><small>分钟</small></article>
    </div>

    <article class="card empty-state">
      <div class="empty-icon">▥</div>
      <h2>暂无可展示的数据</h2>
      <p>统计持久化会在 BLE 语音和按键主路径通过 Windows 验证后接入。</p>
    </article>
  </section>
</template>
