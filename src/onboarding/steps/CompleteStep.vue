<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  checks: Array<{ label: string; ok: boolean }>;
}>();

defineEmits<{ fix: [] }>();

const allOk = computed(() => props.checks.every((check) => check.ok));
const failingLabel = computed(
  () => props.checks.find((check) => !check.ok)?.label ?? "",
);
</script>

<template>
  <section class="onboarding-step">
    <h1>设置完成</h1>
    <p class="onboarding-lede">开始使用前，再确认一次关键条件都还在。</p>

    <div class="onboarding-callout">
      <ul class="onboarding-check-list">
        <li v-for="check in checks" :key="check.label">
          <span :class="check.ok ? 'onboarding-ok' : 'onboarding-error'">{{ check.ok ? "✓" : "×" }}</span>
          <span>{{ check.label }}</span>
        </li>
      </ul>
      <p v-if="!allOk" class="onboarding-muted">有设置不再可用，修好后再点「开始使用」。</p>
    </div>

    <div v-if="!allOk" class="onboarding-callout warn">
      <p>需要先处理：{{ failingLabel }}。</p>
      <button class="primary-button" type="button" @click="$emit('fix')">去修复</button>
    </div>

    <p class="onboarding-muted">完成后随时可以在「设置」里重新运行设置向导。</p>
  </section>
</template>
