<script setup lang="ts">
import { computed } from "vue";
import StatusRow from "../../components/onboarding/StatusRow.vue";

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

    <div class="onboarding-rows">
      <StatusRow
        v-for="check in checks"
        :key="check.label"
        :state="check.ok ? 'ok' : 'warn'"
        :title="check.label"
        hide-state
      />
    </div>

    <div v-if="!allOk" class="onboarding-card warn">
      <h4>有设置不再可用，修好后再点「开始使用」</h4>
      <p>需要先处理：{{ failingLabel }}。</p>
      <div class="onboarding-chips">
        <button class="onboarding-chip strong" type="button" @click="$emit('fix')">
          去修复
        </button>
      </div>
    </div>

    <p class="onboarding-muted">完成后随时可以在「设置」里重新运行设置向导。</p>
  </section>
</template>
