<script setup lang="ts">
import { onMounted, ref } from "vue";

/**
 * 「全按键支持」开启前的确认弹窗，**每次开启都先弹出**（内部仍沿用
 * rc003/enhanced-capture 代码标识）。
 *
 * 2026-10-03 Andy 定稿：每次打开都要重新弹窗 + 重新授权——每次开启都会弹出
 * Windows 授权窗口，所以弹窗必须每次都出现；父页面在开启方向无条件调用，
 * 不再依赖任何授权判据（旧口径「只在这次开启会触发系统授权时才弹」已废除）。
 * 也不记「已读过」：一次性标记存于 localStorage，重装/升级后仍然存活，
 * 会导致「重装后弹窗消失」（2026-09-27 用户报告）。
 *
 * 文案口径（2026-09-26 / 2026-10-03 用户确认）：只说用户能懂的事——
 * 做什么、每次开启都要授权、杀毒软件可能拦截造成失灵、防作弊游戏可能冲突、
 * 只读按键不收集数据。术语红线与写法约定见 `docs/product-copy.md`
 * （本组件不再重复维护内部术语黑名单）。
 *
 * 纯展示组件：不持有任何状态，确认与否完全交给父页面决定
 * （父页面负责真正执行开启）。
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
    <h3 id="capture-confirm-title">开启“全按键支持”？</h3>
    <p class="capture-confirm-intro">
      开启后，无线麦会直接读取遥控器上已配置的按键，避免影响物理键盘上的同名按键；返回 / 音量+ / 音量−也需要开启后才能使用。每次开启都会弹出 Windows 授权窗口，请选择“是”；关闭开关即停。
    </p>
    <p class="capture-confirm-note">
      个别杀毒软件可能把这个功能当成风险，拦截或关闭它，导致按键失灵、开启失败。
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
