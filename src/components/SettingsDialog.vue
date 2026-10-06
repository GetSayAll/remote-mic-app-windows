<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";

const props = withDefaults(defineProps<{
  title: string;
  description?: string;
  busy?: boolean;
  closeLabel?: string;
}>(), {
  description: "",
  busy: false,
  closeLabel: "关闭",
});

const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLElement | null>(null);

function close(): void {
  if (!props.busy) emit("close");
}

onMounted(() => {
  void nextTick(() => {
    const first = dialog.value?.querySelector<HTMLElement>(
      "input:not([disabled]), select:not([disabled]), button:not([disabled]), [tabindex='0']",
    );
    (first ?? dialog.value)?.focus();
  });
});
</script>

<template>
  <div class="settings-dialog-backdrop" @mousedown.self="close">
    <section
      ref="dialog"
      class="settings-dialog"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      tabindex="-1"
      @keydown.esc.prevent="close"
    >
      <header class="settings-dialog-header">
        <div>
          <h2>{{ title }}</h2>
          <p v-if="description" class="muted">{{ description }}</p>
        </div>
        <button
          type="button"
          class="dialog-close-button"
          :aria-label="closeLabel"
          :title="closeLabel"
          :disabled="busy"
          @click="close"
        >
          ×
        </button>
      </header>
      <div class="settings-dialog-body">
        <slot />
      </div>
      <footer v-if="$slots.actions" class="settings-dialog-actions">
        <slot name="actions" />
      </footer>
    </section>
  </div>
</template>

<style scoped>
.settings-dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: grid;
  place-items: center;
  padding: 20px;
  background: color-mix(in srgb, #10141b 48%, transparent);
}
.settings-dialog {
  width: min(720px, calc(100vw - 40px));
  max-height: min(760px, calc(100vh - 40px));
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  overflow: hidden;
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  color: var(--text-primary);
  background: var(--card);
  box-shadow: 0 24px 80px color-mix(in srgb, #000 34%, transparent);
}
.settings-dialog:focus { outline: none; }
.settings-dialog-header,
.settings-dialog-actions {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
  padding: 16px 18px;
  background: var(--card);
}
.settings-dialog-header { border-bottom: 1px solid var(--border); }
.settings-dialog-header h2 { margin: 0; font-size: 17px; }
.settings-dialog-header p { margin: 4px 0 0; font-size: 13px; }
.settings-dialog-body { min-height: 0; padding: 18px; overflow: auto; }
.settings-dialog-actions {
  align-items: center;
  justify-content: flex-end;
  border-top: 1px solid var(--border);
}
.dialog-close-button {
  width: 30px;
  min-width: 30px;
  min-height: 30px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  color: var(--muted);
  background: transparent;
  font-size: 22px;
  line-height: 1;
}
.dialog-close-button:hover { color: var(--text-primary); background: var(--surface-subtle); }
</style>
