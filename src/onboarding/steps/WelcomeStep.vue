<template>
  <section class="onboarding-step">
    <h1>欢迎使用无线麦 SayAll</h1>
    <p class="onboarding-lede">
      接下来会完成真实的设置和验证，不是走一遍页面教程。每一步都确认可用后才会放行。
    </p>

    <div class="onboarding-card">
      <h4>开始前请准备好：</h4>
      <ul class="onboarding-list">
        <li>小米蓝牙语音遥控器 2 或 2 Pro（只支持这两种型号）</li>
        <li>电脑的蓝牙已打开</li>
      </ul>
    </div>

    <div class="onboarding-card accent">
      <h4>第一次使用：遥控器怎么连电脑</h4>
      <ul class="onboarding-numlist">
        <li>
          <span class="onboarding-num">1</span>
          <span>长按遥控器上的「TV」键约 3 秒，直到指示灯开始闪烁（进入配对模式）。</span>
        </li>
        <li>
          <span class="onboarding-num">2</span>
          <span>
            电脑上打开：设置 → 蓝牙和其他设备 → 添加设备 → 蓝牙。列表里如果没有「小米蓝牙语音遥控器」，
            先点开「高级选项」（或“更多蓝牙选项”）再等它刷新。
          </span>
        </li>
        <li>
          <span class="onboarding-num">3</span>
          <span>
            选择「小米蓝牙语音遥控器」完成配对。配不上，或遥控器以前配过别的设备：
            同时长按「主页」+「菜单」重置蓝牙，再回到第 1 步重试。
          </span>
        </li>
      </ul>
    </div>

    <p class="onboarding-muted">
      设置中途可以随时退出；下次打开会回到这里继续。全程不会修改系统默认音频设备。
    </p>
  </section>
</template>
