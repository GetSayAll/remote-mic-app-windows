<script setup lang="ts">
import { onMounted, ref } from "vue";

/**
 * 「全按键支持」首次开启前的一次性确认弹窗（内部仍叫增强捕获/RC003 三键捕获）。
 *
 * 文案口径（2026-09-26 用户确认）：只说用户能懂的事——
 * 做什么、系统会弹窗问一次、升级/重装后要重开、防作弊游戏可能冲突、
 * 只读按键不收集数据；不出现 HID 宿主 / UAC / 计划任务 / 注入等内部术语。
 *
 * 纯展示组件：不持有任何状态，确认与否完全交给父页面决定
 * （父页面负责写"已确认"标记与真正执行开启）。
 */
const emit = defineEmits<{ confirm: []; close: [] }>();
const dialog = ref<HTMLDialogElement | null>(null);

onMounted(() => {
  // jsdom 可能没有 showModal，退回 open 属性（与 RegisteredAppsDialog 同一套写法）。
  if (dialog.value?.showModal) dialog.value.showModal();
  else dialog.value?.setAttribute("open", "");
});
</script>

<template>
  <dialog
    ref="dialog"
    class="capture-confirm-dialog"
    aria-labelledby="capture-confirm-title"
    @cancel.prevent="emit('close')"
  >
    <h3 id="capture-confirm-title">开启「全按键支持」？</h3>
    <p class="capture-confirm-intro">
      开启后，无线麦就能读到遥控器上的返回 / 音量+ / 音量−
      这三个键（Windows 平时看不见它们），并按你在无线麦里设置的动作用它们。首次开启时系统会弹窗询问，请选择「是」；之后每次打开无线麦都自动生效，关闭开关即停。
    </p>
    <p class="capture-confirm-note">
      升级或重装无线麦后，这个开关需要重新打开一次（会再弹一次询问）。
    </p>
    <p class="capture-confirm-note">
      个别游戏带有防作弊保护，可能和这个功能合不来：表现为按键失灵、游戏打不开等。玩这类游戏前，建议先把全按键支持关掉。
    </p>
    <p class="capture-confirm-privacy">
      无线麦只读取这个遥控器的按键，不收集、不上传任何其他信息。随时可以关闭，关闭后一切恢复原样。
    </p>
    <footer class="capture-confirm-footer">
      <button type="button" class="secondary-button" @click="emit('close')">取消</button>
      <button type="button" class="primary-button" @click="emit('confirm')">开启</button>
    </footer>
  </dialog>
</template>

<style scoped>
.capture-confirm-dialog {
  width: min(540px, calc(100vw - 40px));
  box-sizing: border-box;
  padding: 20px 22px;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  background: var(--surface-canvas);
  color: var(--text-primary);
  overflow: auto;
}
.capture-confirm-dialog::backdrop {
  background: #0008;
}
.capture-confirm-dialog h3 {
  margin: 0 0 12px;
  font-size: 15px;
  font-weight: 500;
}
.capture-confirm-intro {
  margin: 0 0 12px;
  font-size: 13px;
  line-height: 1.6;
}
.capture-confirm-note {
  margin: 0 0 8px;
  padding: 10px 12px;
  border-radius: 6px;
  background: var(--warning-surface);
  color: var(--warning-text);
  font-size: 13px;
  line-height: 1.6;
}
.capture-confirm-privacy {
  margin: 0 0 14px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
}
.capture-confirm-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
