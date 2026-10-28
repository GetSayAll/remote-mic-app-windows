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
  label: "开通 Plus 会员",
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
  min-height: 32px;
  padding: 5px 14px;
  border: 0;
  border-radius: 999px;
  background: var(--membership-pink, #f53f8f);
  /* Keep the label white on the brand pink pill. */
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
  width: 14px;
  height: 14px;
}
</style>
