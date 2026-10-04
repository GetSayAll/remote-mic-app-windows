<script setup lang="ts">
import { buttonLabel, type RemoteButton } from "../../lib/bridge";

defineProps<{
  observed: Array<{ button: RemoteButton; count: number }>;
  voiceKeyMistake: boolean;
}>();
</script>

<template>
  <section class="onboarding-step">
    <h1>按一下遥控器的普通按键</h1>
    <p class="onboarding-lede">
      请用遥控器上除语音键以外的按键，按 3 个不同的键（比如 主页 / OK / 方向键）。
      这一步只看按键能不能用，不会执行你以前配置的动作。
    </p>

    <div v-if="observed.length > 0" class="onboarding-callout">
      <p>已收到 {{ observed.length }} 个不同按键：</p>
      <ul class="onboarding-device-list">
        <li v-for="item in observed" :key="item.button">
          <span>{{ buttonLabel(item.button) }}<template v-if="item.count > 1"> × {{ item.count }}</template></span>
        </li>
      </ul>
      <p v-if="observed.length < 3" class="onboarding-muted">
        再按 {{ 3 - observed.length }} 个不同的普通按键。
      </p>
      <p v-else class="onboarding-ok">三个按键都收到了，这一项可以继续了。</p>
    </div>
    <p v-else class="onboarding-muted">还没有收到按键；按一下遥控器上的任意普通按键开始。</p>

    <div v-if="voiceKeyMistake" class="onboarding-callout warn">
      这是语音键。请按普通按键（比如主页键或方向键）。
    </div>
  </section>
</template>
