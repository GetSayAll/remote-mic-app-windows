<script setup lang="ts">
import { computed } from "vue";
import { buttonLabel, type RemoteButton } from "../../lib/bridge";
import { t } from "../../lib/i18n";

const props = defineProps<{
  observed: Array<{ button: RemoteButton; count: number }>;
  voiceKeyMistake: boolean;
  observeOnly?: boolean;
}>();

/** 固定展示顺序：电源 / 主页 / 返回 / 菜单 / TV / 确定 / 方向 / 音量 / 静音。 */
const BUTTON_ORDER: RemoteButton[] = [
  "power",
  "home",
  "back",
  "menu",
  "tv",
  "ok",
  "up",
  "down",
  "left",
  "right",
  "volume_up",
  "volume_down",
  "volume_mute",
];

const observedButtons = computed(() => new Set(props.observed.map((item) => item.button)));
const displayedButtons = computed(() => props.observeOnly
  ? [...BUTTON_ORDER.filter((button) => button !== "menu" && button !== "tv"), "youtube", "netflix", "input"] as RemoteButton[]
  : BUTTON_ORDER);
const distinctCount = computed(() => Math.min(3, observedButtons.value.size));
</script>

<template>
  <section class="onboarding-step">
    <h1>{{ observeOnly ? t("查看遥控器按键") : t("按一下遥控器的普通按键") }}</h1>
    <p v-if="observeOnly" class="onboarding-lede">
      {{ t("普通按键仅供查看，可能仍会触发 Windows 的原有操作。你可以直接继续。") }}
    </p>
    <p v-else class="onboarding-lede">
      {{ t("请用遥控器上除语音键以外的按键，按 3 个不同的键（比如 主页 / OK / 方向键）。这一步只看按键能不能用，不会执行你以前配置的动作。") }}
    </p>

    <div class="onboarding-btnchips">
      <span
        v-for="button in displayedButtons"
        :key="button"
        class="onboarding-bchip"
        :class="{ hit: observedButtons.has(button) }"
      >
        <span class="dot" aria-hidden="true"></span>{{ buttonLabel(button) }}
      </span>
    </div>

    <div v-if="!observeOnly" class="onboarding-dots">
      <span v-for="index in 3" :key="index" class="dot" :class="{ on: index <= distinctCount }"></span>
      <span v-if="observed.length < 3 && observed.length > 0">{{
        t("再按 {count} 个不同的普通按键。", { count: 3 - observed.length })
      }}</span>
      <span v-else-if="observed.length >= 3">{{ t("三个按键都收到了，这一项可以继续了。") }}</span>
      <span v-else>{{ t("还没有收到按键；按一下遥控器上的任意普通按键开始。") }}</span>
    </div>

    <div v-if="!observeOnly && voiceKeyMistake" class="onboarding-card warn">
      <h4>{{ t("这是语音键") }}</h4>
      <p>{{ t("请按普通按键（比如主页键或方向键）。") }}</p>
    </div>
  </section>
</template>
