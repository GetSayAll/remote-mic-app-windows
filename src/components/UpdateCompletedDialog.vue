<script setup lang="ts">
import { onMounted, ref } from "vue";

/**
 * 「无线麦已更新」提示（2026-10-19，对齐 Mac `showUpdateCompletedAlert`）。
 *
 * 只在「本次启动前刚完成更新」时出现一次：版本号由 Rust 侧取走即清空，
 * 页面重新挂载不会再弹。纯展示组件，点「好」或按 Esc 都只发 `close`，
 * 由父级清掉版本号来卸载本组件。
 */
const props = defineProps<{ version: string }>();
const emit = defineEmits<{ close: [] }>();
const dialog = ref<HTMLDialogElement | null>(null);

onMounted(() => {
  // jsdom 可能没有 showModal，退回 open 属性（与既有弹窗同一套写法）。
  if (dialog.value?.showModal) dialog.value.showModal();
  else dialog.value?.setAttribute("open", "");
});
</script>

<template>
  <dialog
    ref="dialog"
    class="update-completed-dialog"
    aria-labelledby="update-completed-title"
    @cancel.prevent="emit('close')"
  >
    <h3 id="update-completed-title">无线麦已更新</h3>
    <p class="update-completed-message">已成功更新到版本 {{ props.version }}。</p>
    <footer class="update-completed-footer">
      <button type="button" class="primary-button" @click="emit('close')">好</button>
    </footer>
  </dialog>
</template>

<style scoped>
.update-completed-dialog {
  width: min(380px, calc(100vw - 40px));
  box-sizing: border-box;
  padding: 20px 22px;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  background: var(--surface-canvas);
  color: var(--text-primary);
}
.update-completed-dialog::backdrop {
  background: #0008;
}
.update-completed-dialog h3 {
  margin: 0 0 10px;
  font-size: 15px;
  font-weight: 500;
}
.update-completed-message {
  margin: 0 0 18px;
  font-size: 13px;
  line-height: 1.6;
}
.update-completed-footer {
  display: flex;
  justify-content: flex-end;
}
</style>
