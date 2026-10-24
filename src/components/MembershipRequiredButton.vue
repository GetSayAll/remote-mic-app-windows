<script setup lang="ts">
/**
 * 通用会员入口按钮。
 *
 * 页面自己判断门禁（会员快照、常用语 access、未来的键位方案 access），
 * 组件只负责在未开通时显示统一的皇冠 CTA，事件交给宿主决定去哪开通。
 */
import CrownIcon from "./CrownIcon.vue";
import { t } from "../lib/i18n";

withDefaults(defineProps<{ label?: string }>(), {
  label: "点击开通会员功能",
});

defineEmits<{ open: [] }>();
</script>

<template>
  <button class="membership-required-button" type="button" @click="$emit('open')">
    <CrownIcon class="membership-required-icon" />
    <span>{{ t(label) }}</span>
  </button>
</template>

<style scoped>
.membership-required-button {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 30px;
  padding: 4px 11px;
  border: 0;
  border-radius: 999px;
  background: var(--accent, #e11d63);
  /* `--accent-text` is the accent colour used for text, not a foreground on
     the accent fill. Using it here makes the label the same pink as the pill. */
  color: #fff;
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  cursor: pointer;
}

.membership-required-button:hover {
  filter: brightness(0.92);
}

.membership-required-icon {
  width: 13px;
  height: 13px;
}
</style>
