<script setup lang="ts">
import {
  hideMainWindowFromTitleBar,
  minimizeMainWindow,
  toggleMaximizeMainWindow,
} from "../lib/bridge";
import { t } from "../lib/i18n";

/**
 * 自定义窗口标题栏（2026-11-06 用户要求）：
 * - 颜色始终与侧栏一致，不显示应用 logo 或窗口文字；
 * - 只剩可拖拽区域和右侧的三枚窗口按钮；
 * - 关闭与 Ctrl+W / 原生 X 同一语义：收进托盘，不退出进程。
 */
function run(action: () => Promise<void>): void {
  void action().catch(() => undefined);
}
</script>

<template>
  <header class="window-titlebar">
    <div
      class="window-titlebar-drag"
      data-tauri-drag-region
      @dblclick="run(toggleMaximizeMainWindow)"
    ></div>
    <div class="window-controls">
      <button
        class="window-control"
        type="button"
        :aria-label="t('最小化')"
        :title="t('最小化')"
        @click="run(minimizeMainWindow)"
      >
        <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <path d="M2 5h6" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
      <button
        class="window-control"
        type="button"
        :aria-label="t('最大化/还原')"
        :title="t('最大化/还原')"
        @click="run(toggleMaximizeMainWindow)"
      >
        <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <rect
            x="2.2"
            y="2.2"
            width="5.6"
            height="5.6"
            rx="0.4"
            stroke="currentColor"
            stroke-width="1"
          />
        </svg>
      </button>
      <button
        class="window-control window-control-close"
        type="button"
        :aria-label="t('关闭窗口')"
        :title="t('关闭窗口')"
        @click="run(hideMainWindowFromTitleBar)"
      >
        <svg viewBox="0 0 10 10" fill="none" aria-hidden="true">
          <path
            d="M2.2 2.2l5.6 5.6M7.8 2.2L2.2 7.8"
            stroke="currentColor"
            stroke-width="1"
          />
        </svg>
      </button>
    </div>
  </header>
</template>

<style scoped>
/* 与侧栏共用 --sidebar；标题栏跨整行，左段自然与侧栏连续。 */
.window-titlebar {
  flex: 0 0 var(--titlebar-height);
  height: var(--titlebar-height);
  display: flex;
  align-items: stretch;
  background: var(--sidebar);
  user-select: none;
}

.window-titlebar-drag {
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
}

.window-controls {
  display: flex;
  flex: 0 0 auto;
  height: 100%;
}

.window-control {
  width: 46px;
  height: 100%;
  display: grid;
  place-items: center;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-nav);
  cursor: default;
}

.window-control:hover {
  background: var(--nav-hover);
}

.window-control:active {
  background: var(--border);
}

.window-control:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.window-control-close:hover {
  background: #e81123;
  color: #ffffff;
}

.window-control svg {
  width: 10px;
  height: 10px;
  display: block;
}
</style>
