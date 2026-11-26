<script setup lang="ts">
/**
 * 「配置有问题？」二维码弹窗（2026-11-26 用户要求）。
 *
 * Onboarding 页脚与设置页「首次设置」共用：只展示官方抖音 / 小红书账号二维码，
 * 点「关闭」或按 Esc 由父级卸载。二维码来自本机静态资源
 * （`public/douyin-qr.png` / `public/xhs-qr.png`），不请求二维码服务。
 */
import { onMounted, ref } from "vue";
import { t } from "../lib/i18n";

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
    class="help-qr-dialog"
    aria-labelledby="help-qr-title"
    @cancel.prevent="emit('close')"
  >
    <h3 id="help-qr-title">{{ t("配置有问题？") }}</h3>
    <p class="help-qr-intro">
      {{ t("扫码关注抖音或小红书，查看设置教程。") }}
    </p>
    <div class="help-qr-cards">
      <figure class="help-qr-card">
        <img
          class="help-qr-image help-qr-image--dark"
          src="/douyin-qr.png"
          :alt="t('抖音「无线麦 SayAll.App」二维码')"
        />
        <figcaption>
          <strong>{{ t("抖音 · 无线麦 SayAll.App") }}</strong>
          <span>{{ t("用抖音扫一扫") }}</span>
        </figcaption>
      </figure>
      <figure class="help-qr-card">
        <img
          class="help-qr-image"
          src="/xhs-qr.png"
          :alt="t('小红书「无线麦 App」二维码')"
        />
        <figcaption>
          <strong>{{ t("小红书 · 无线麦 App") }}</strong>
          <span>{{ t("用小红书扫一扫") }}</span>
        </figcaption>
      </figure>
    </div>
    <footer class="help-qr-footer">
      <button type="button" class="secondary-button" @click="emit('close')">{{ t("关闭") }}</button>
    </footer>
  </dialog>
</template>

<style scoped>
.help-qr-dialog {
  width: min(460px, calc(100vw - 40px));
  box-sizing: border-box;
  padding: 20px 22px;
  border: 1px solid var(--border-strong);
  border-radius: 10px;
  background: var(--surface-canvas);
  color: var(--text-primary);
}
.help-qr-dialog::backdrop {
  background: #0008;
}
.help-qr-dialog h3 {
  margin: 0 0 8px;
  font-size: 15px;
  font-weight: 500;
}
.help-qr-intro {
  margin: 0 0 16px;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.6;
}
.help-qr-cards {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}
.help-qr-card {
  min-width: 0;
  margin: 0;
}
.help-qr-image {
  display: block;
  width: 100%;
  aspect-ratio: 1;
  box-sizing: border-box;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: #ffffff;
  object-fit: contain;
}
.help-qr-image--dark {
  background: #0b0b0b;
}
.help-qr-card figcaption {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 8px;
  text-align: center;
}
.help-qr-card strong {
  font-size: 13px;
  font-weight: 600;
  line-height: 1.4;
}
.help-qr-card span {
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.4;
}
.help-qr-footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 18px;
}

/* 窄窗：两张二维码改为上下排列，避免缩得太小扫不出来。 */
@media (max-width: 420px) {
  .help-qr-cards {
    grid-template-columns: minmax(0, 1fr);
    max-width: 220px;
    margin: 0 auto;
  }
}
</style>
