<script setup lang="ts">
import { computed } from "vue";
import { buttonLabel, type RemoteButton } from "../../lib/bridge";
import { t } from "../../lib/i18n";
import { ONBOARDING_CONTROL_BUTTONS, isOnboardingControlButton } from "../control-buttons";

const props = defineProps<{
  observed: Array<{ button: RemoteButton; count: number }>;
  voiceKeyMistake: boolean;
}>();

/** 固定展示顺序（见 `control-buttons.ts`）：静音键不参与本步骤。 */
const BUTTON_ORDER = ONBOARDING_CONTROL_BUTTONS;

const observedButtons = computed(
  () =>
    new Set(
      props.observed
        .filter((item) => isOnboardingControlButton(item.button))
        .map((item) => item.button),
    ),
);
const observedCount = computed(() => observedButtons.value.size);
const distinctCount = computed(() => Math.min(3, observedCount.value));
</script>

<template>
  <section class="onboarding-step">
    <h1>{{ t("按一下遥控器的普通按键") }}</h1>
    <p class="onboarding-lede">
      {{ t("请用遥控器上除语音键以外的按键，按 3 个不同的键（比如 主页 / OK / 方向键）。这一步只看按键能不能用，不会执行你以前配置的动作。") }}
    </p>

    <div class="onboarding-btnchips">
      <span
        v-for="button in BUTTON_ORDER"
        :key="button"
        class="onboarding-bchip"
        :class="{ hit: observedButtons.has(button) }"
      >
        <span class="dot" aria-hidden="true"></span>{{ buttonLabel(button) }}
      </span>
    </div>

    <div class="onboarding-dots">
      <span v-for="index in 3" :key="index" class="dot" :class="{ on: index <= distinctCount }"></span>
      <span v-if="observedCount < 3 && observedCount > 0">{{
        t("再按 {count} 个不同的普通按键。", { count: 3 - observedCount })
      }}</span>
      <span v-else-if="observedCount >= 3">{{ t("三个按键都收到了，这一项可以继续了。") }}</span>
      <span v-else>{{ t("还没有收到按键；按一下遥控器上的任意普通按键开始。") }}</span>
    </div>

    <div v-if="voiceKeyMistake" class="onboarding-card warn">
      <h4>{{ t("这是语音键") }}</h4>
      <p>{{ t("请按普通按键（比如主页键或方向键）。") }}</p>
    </div>
  </section>
</template>
