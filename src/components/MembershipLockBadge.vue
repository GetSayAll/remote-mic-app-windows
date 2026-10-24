<script setup lang="ts">
import { t } from "../lib/i18n";

/**
 * 会员锁标：任何需要 Plus 的功能都可以复用。
 * 只负责展示，不读取会员状态、不发起命令。
 */
withDefaults(defineProps<{ label?: string }>(), {
  label: "需要 Plus 会员",
});
</script>

<template>
  <span class="membership-lock-badge">
    <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="5" y="10.5" width="14" height="9.5" rx="2" stroke="currentColor" stroke-width="1.8" />
      <path d="M8.2 10.5V8a3.8 3.8 0 1 1 7.6 0v2.5" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" />
    </svg>
    <span>{{ t(label) }}</span>
  </span>
</template>

<style scoped>
.membership-lock-badge {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 8px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent, #e11d63) 12%, transparent);
  color: var(--accent, #e11d63);
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
}

.membership-lock-badge svg {
  width: 12px;
  height: 12px;
}
</style>
