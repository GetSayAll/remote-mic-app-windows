<script setup lang="ts">
import { nextTick, onMounted, onUnmounted, ref } from "vue";
import { SHARE_WEBSITE_URL } from "../lib/bridge";

const emit = defineEmits<{ close: [] }>();
const closeButton = ref<HTMLButtonElement | null>(null);
const copyState = ref<"idle" | "success" | "failure">("idle");
let previousFocus: HTMLElement | null = null;
let previousOverflow = "";

async function copyLink(): Promise<void> {
  try {
    if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
    await navigator.clipboard.writeText(SHARE_WEBSITE_URL);
    copyState.value = "success";
  } catch {
    copyState.value = "failure";
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  emit("close");
}

onMounted(() => {
  previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  previousOverflow = document.body.style.overflow;
  document.body.style.overflow = "hidden";
  window.addEventListener("keydown", onKeydown, true);
  void nextTick(() => closeButton.value?.focus());
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown, true);
  document.body.style.overflow = previousOverflow;
  previousFocus?.focus();
});
</script>

<template>
  <div class="share-overlay" @click.self="emit('close')">
    <section
      class="share-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="share-dialog-title"
    >
      <header class="share-header">
        <span class="share-header-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <path
              d="M12 15V3M8 7l4-4 4 4M5 11v7.5A2.5 2.5 0 0 0 7.5 21h9a2.5 2.5 0 0 0 2.5-2.5V11"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
          </svg>
        </span>
        <div class="share-heading">
          <h2 id="share-dialog-title">分享无线麦 SayAll</h2>
          <p>邀请朋友发现用遥控器或手机在 Windows 上进行语音输入的便捷方式。</p>
        </div>
        <button
          ref="closeButton"
          type="button"
          class="share-close"
          aria-label="关闭分享"
          @click="emit('close')"
        >
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              d="M6 6l12 12M18 6L6 18"
              stroke="currentColor"
              stroke-width="1.9"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </header>

      <div class="share-card">
        <div class="share-qr">
          <img src="/app-share-qr.png" alt="无线麦 SayAll 官网二维码" />
        </div>
        <div class="share-copy">
          <h3>扫码或复制链接</h3>
          <p>可用手机扫描二维码，也可复制同一个官网链接。</p>
          <code>{{ SHARE_WEBSITE_URL }}</code>
          <button type="button" class="primary-button share-copy-button" @click="copyLink">
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path
                d="M8.5 8.5h10v10h-10zM5.5 15.5h-2v-10h10v2"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            复制链接
          </button>
          <p
            v-if="copyState !== 'idle'"
            class="share-copy-result"
            :class="copyState"
            role="status"
            aria-live="polite"
          >
            {{ copyState === "success" ? "链接已复制" : "链接复制失败" }}
          </p>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.share-overlay {
  position: fixed;
  z-index: 1000;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(16, 19, 28, 0.34);
  backdrop-filter: blur(5px);
}

.share-dialog {
  width: min(560px, calc(100vw - 48px));
  padding: 20px;
  border: 1px solid var(--border);
  border-radius: 20px;
  background: var(--card);
  box-shadow: 0 24px 80px rgba(17, 20, 32, 0.28);
}

.share-header {
  display: flex;
  align-items: flex-start;
  gap: 14px;
}

.share-header-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  flex: 0 0 auto;
  color: var(--accent);
}

.share-header-icon svg {
  width: 24px;
  height: 24px;
}

.share-heading {
  min-width: 0;
  flex: 1 1 auto;
}

.share-heading h2 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.3;
}

.share-heading p {
  margin: 3px 0 0;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.45;
}

.share-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  flex: 0 0 auto;
  padding: 0;
  border: 0;
  border-radius: 8px;
  color: var(--muted);
  background: transparent;
  cursor: pointer;
}

.share-close:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.share-close svg {
  width: 16px;
  height: 16px;
}

.share-card {
  display: flex;
  align-items: center;
  gap: 20px;
  margin-top: 18px;
  padding-top: 18px;
  border-top: 1px solid var(--border);
}

.share-qr {
  width: 160px;
  height: 160px;
  flex: 0 0 auto;
  padding: 8px;
  border-radius: 12px;
  background: #ffffff;
}

.share-qr img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  image-rendering: pixelated;
}

.share-copy {
  min-width: 0;
  flex: 1 1 auto;
}

.share-copy h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.3;
}

.share-copy p {
  margin: 6px 0 0;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.5;
}

.share-copy code {
  display: block;
  margin-top: 10px;
  color: var(--muted);
  font-family: "Cascadia Mono", Consolas, monospace;
  font-size: 12px;
  line-height: 1.45;
  overflow-wrap: anywhere;
  user-select: text;
}

.share-copy-button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-top: 12px;
}

.share-copy-button svg {
  width: 15px;
  height: 15px;
}

.share-copy-result {
  margin: 8px 0 0;
  font-size: 12px;
  font-weight: 600;
}

.share-copy-result.success {
  color: var(--success-text);
}

.share-copy-result.failure {
  color: var(--error-text);
}

@media (max-width: 620px) {
  .share-card {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
