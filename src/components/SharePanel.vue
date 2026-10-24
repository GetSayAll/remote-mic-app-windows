<script setup lang="ts">
import { ref } from "vue";
import { SHARE_WEBSITE_URL } from "../lib/bridge";

const expanded = ref(false);
const copyState = ref<"idle" | "success" | "failure">("idle");

function toggleExpanded(): void {
  expanded.value = !expanded.value;
  if (!expanded.value) copyState.value = "idle";
}

async function copyLink(): Promise<void> {
  try {
    if (!navigator.clipboard?.writeText) throw new Error("clipboard unavailable");
    await navigator.clipboard.writeText(SHARE_WEBSITE_URL);
    copyState.value = "success";
  } catch {
    copyState.value = "failure";
  }
}
</script>

<template>
  <section class="share-panel" aria-labelledby="profile-share-title">
    <header class="share-panel-header">
      <span class="share-panel-icon" aria-hidden="true">
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
      <div class="share-panel-heading">
        <h2 id="profile-share-title">分享无线麦 SayAll</h2>
        <p>邀请朋友发现用遥控器或手机在 Windows 上进行语音输入的便捷方式。</p>
      </div>
      <button
        type="button"
        class="share-toggle"
        :aria-expanded="expanded"
        aria-controls="profile-share-content"
        @click="toggleExpanded"
      >
        <svg v-if="!expanded" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2M20 14v2M14 20h6v-2M18 18h2"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <svg v-else viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M6 15l6-6 6 6"
            stroke="currentColor"
            stroke-width="1.9"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <span>{{ expanded ? "收起" : "显示二维码" }}</span>
      </button>
    </header>

    <div v-if="expanded" id="profile-share-content" class="share-panel-content">
      <div class="share-divider"></div>
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
    </div>
  </section>
</template>

<style scoped>
.share-panel {
  padding: 16px;
  border: 1px solid var(--border);
  border-radius: 20px;
  background: var(--card);
  box-shadow: 0 8px 30px var(--shadow-card);
}

.share-panel-header {
  display: flex;
  align-items: center;
  gap: 14px;
}

.share-panel-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  flex: 0 0 auto;
  color: var(--accent);
}

.share-panel-icon svg {
  width: 24px;
  height: 24px;
}

.share-panel-heading {
  min-width: 0;
  flex: 1 1 auto;
}

.share-panel-heading h2 {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
  line-height: 1.3;
}

.share-panel-heading p {
  margin: 3px 0 0;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.45;
}

.share-toggle {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-height: 30px;
  flex: 0 0 auto;
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-control);
  background: var(--surface-control);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}

.share-toggle:hover {
  border-color: var(--accent-border);
  background: var(--surface-hover);
}

.share-toggle svg {
  width: 14px;
  height: 14px;
  flex: 0 0 auto;
}

.share-panel-content {
  margin-top: 14px;
}

.share-divider {
  margin-bottom: 14px;
  border-top: 1px solid var(--border);
}

.share-card {
  display: flex;
  align-items: center;
  gap: 20px;
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
  .share-panel-header {
    align-items: flex-start;
    flex-wrap: wrap;
  }

  .share-toggle {
    margin-left: 48px;
  }

  .share-card {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
