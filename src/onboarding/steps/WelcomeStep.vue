<script setup lang="ts">
/**
 * 第①步：首次配对说明（2026-10-13 现场定稿文案）。
 * 「打开蓝牙设置」入口放在第 2 条下方（Andy 指定位置）：点击只唤起系统设置页，
 * 不写"已打开…"提示（由父级 `openBluetoothSettingsFromWelcome` 保证）。
 */
defineEmits<{ openBluetoothSettings: [] }>();
</script>

<template>
  <section class="onboarding-step">
    <h1>欢迎使用无线麦 SayAll</h1>
    <p class="onboarding-lede">
      接下来会完成真实的设置和验证，不是走一遍页面教程。每一步都确认可用后才会放行。
    </p>

    <div class="onboarding-card accent">
      <h4>首次使用：遥控器怎么连电脑</h4>
      <ul class="onboarding-numlist">
        <li>
          <span class="onboarding-num">1</span>
          <span>准备好：小米蓝牙语音遥控器 2 或 2 Pro（目前只支持这两种型号），并确保它有电。</span>
        </li>
        <li>
          <span class="onboarding-num">2</span>
          <span>
            在电脑上打开蓝牙设置（可直接点击下方按钮），打开电脑的蓝牙开关后点击「添加设备」然后选择「蓝牙」选项。
          </span>
        </li>
        <li class="onboarding-numlist-action">
          <button class="onboarding-chip" type="button" @click="$emit('openBluetoothSettings')">
            打开蓝牙设置
          </button>
        </li>
        <li>
          <span class="onboarding-num">3</span>
          <span>
            长按「TV」键约 3 秒，直到指示灯（小米蓝牙语音遥控器 2 Pro 底部）开始闪烁（进入配对模式）。
          </span>
        </li>
        <li>
          <span class="onboarding-num">4</span>
          <span>
            同时长按「主页」+「菜单」直到指示灯快速闪烁——重置遥控器的蓝牙并重新进入配对模式。
          </span>
        </li>
        <li>
          <span class="onboarding-num">5</span>
          <span>
            选中「小米蓝牙语音遥控器」，等待配对完成，列表里找不到设备时，先点开「显示所有设备」。
          </span>
        </li>
      </ul>
      <p>配对失败，或遥控器以前配对过别的设备：回到第 4 步重置蓝牙后重新配对。</p>
    </div>
  </section>
</template>
