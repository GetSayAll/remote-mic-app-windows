<script setup lang="ts">
/**
 * 单选/可点击卡片（设计稿 2026-10-05）：设备行、工具卡片、端点行共用。
 * 整卡是一个按钮（键盘可达）；`trailing` 槽放右侧状态文本或动作词，
 * 不允许在其中再放按钮（嵌套交互元素）。
 */
withDefaults(
  defineProps<{
    selected?: boolean;
    disabled?: boolean;
    /** radio=单选圆点；none=无指示器（纯动作行，如设备连接行）。 */
    indicator?: "radio" | "none";
  }>(),
  { selected: false, disabled: false, indicator: "radio" },
);

const emit = defineEmits<{ select: [] }>();
</script>

<template>
  <button
    class="onboarding-option"
    :class="{ selected }"
    type="button"
    :disabled="disabled"
    :aria-pressed="selected"
    @click="emit('select')"
  >
    <span v-if="indicator === 'radio'" class="option-radio" aria-hidden="true"></span>
    <span class="option-body"><slot /></span>
    <span class="option-trailing"><slot name="trailing" /></span>
  </button>
</template>
