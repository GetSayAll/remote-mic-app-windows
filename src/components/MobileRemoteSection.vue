<script setup lang="ts">
import { computed, ref, watch } from "vue";
import CrownIcon from "./CrownIcon.vue";
import {
  approvePhoneRemote,
  clearPhoneRemoteTrustedDevices,
  denyPhoneRemote,
  iosAppStoreUrl,
  openIOSAppStore,
  startPhoneRemoteIos,
  startPhoneRemoteWeb,
  stopPhoneRemoteIos,
  stopPhoneRemoteWeb,
  type NearbyRemoteSnapshot,
  type PhoneRemoteSnapshot,
  type WebRemoteSnapshot,
} from "../lib/bridge";
import { t } from "../lib/i18n";

const props = defineProps<{ snapshot?: PhoneRemoteSnapshot | null }>();
const emit = defineEmits<{ navigate: [page: string] }>();

const emptyIos = (): NearbyRemoteSnapshot => ({
  state: "idle",
  expiresAt: null,
  deviceName: null,
  qrCode: null,
});

const emptyWeb = (): WebRemoteSnapshot => ({
  state: "idle",
  expiresAt: null,
  deviceName: null,
  miniProgramCode: null,
});

const local = ref<PhoneRemoteSnapshot | null>(props.snapshot ?? null);
const busy = ref(false);
const actionError = ref("");
const showWebModal = ref(false);
const showClearConfirm = ref(false);
const appStoreCopied = ref(false);

watch(
  () => props.snapshot,
  (snapshot) => {
    if (snapshot) local.value = snapshot;
  },
  { immediate: true },
);

const visible = computed(() => local.value?.availability === "available");
const gated = computed(() => local.value?.membership !== "active");
const ios = computed<NearbyRemoteSnapshot>(() => local.value?.ios ?? emptyIos());
const web = computed<WebRemoteSnapshot>(() => local.value?.web ?? emptyWeb());
const approval = computed(() => local.value?.approval ?? null);
const trustedCount = computed(() => local.value?.trustedDeviceCount ?? 0);

const iosStatus = computed(() => {
  if (gated.value) return t("尚未开启");
  switch (ios.value.state) {
    case "waiting":
      return t("正在等待手机");
    case "waiting_for_approval":
      return t("等待确认");
    case "connected":
      return t("已连接");
    case "failed":
      return t("无法连接");
    default:
      return t("尚未开启");
  }
});

const iosTone = computed(() => {
  if (gated.value) return "";
  switch (ios.value.state) {
    case "waiting":
    case "waiting_for_approval":
      return "warning";
    case "connected":
      return "success";
    case "failed":
      return "error";
    default:
      return "";
  }
});

const iosAction = computed(() => {
  if (gated.value) return t("点击开通会员功能");
  switch (ios.value.state) {
    case "waiting":
    case "waiting_for_approval":
      return t("取消等待");
    case "connected":
      return t("取消连接");
    case "failed":
      return t("重试");
    default:
      return t("连接");
  }
});

const webStatus = computed(() => {
  if (gated.value) return t("尚未开启");
  switch (web.value.state) {
    case "unavailable":
      return t("尚未开放");
    case "waiting_for_scan":
      return t("等待扫码");
    case "waiting_for_approval":
      return t("等待确认");
    case "connected":
      return t("手机遥控已连接");
    case "expired":
      return t("二维码已过期");
    case "failed":
      return t("连接失败");
    default:
      return t("尚未开启");
  }
});

const webTone = computed(() => {
  if (gated.value) return "";
  switch (web.value.state) {
    case "connected":
      return "success";
    case "expired":
    case "failed":
      return "warning";
    default:
      return "";
  }
});

const webAction = computed(() => {
  if (gated.value) return t("点击开通会员功能");
  if (
    web.value.state === "waiting_for_scan" ||
    web.value.state === "waiting_for_approval" ||
    web.value.state === "connected"
  ) {
    return t("显示二维码");
  }
  return t("连接");
});

const showIosQr = computed(
  () =>
    !gated.value &&
    (ios.value.state === "waiting" || ios.value.state === "waiting_for_approval") &&
    Boolean(ios.value.qrCode),
);

const showWebCode = computed(() =>
  Boolean(web.value.miniProgramCode) &&
  ["waiting_for_scan", "waiting_for_approval"].includes(web.value.state),
);

function applySnapshot(snapshot: PhoneRemoteSnapshot): void {
  local.value = snapshot;
  actionError.value = "";
}

function fail(error: unknown): void {
  console.error("[phone-remote]", error);
  actionError.value = t("操作没有完成，请稍后重试。");
}

async function onIosAction(): Promise<void> {
  if (busy.value) return;
  if (gated.value) {
    emit("navigate", "membership");
    return;
  }
  busy.value = true;
  actionError.value = "";
  try {
    if (ios.value.state === "idle" || ios.value.state === "failed") {
      applySnapshot(await startPhoneRemoteIos());
    } else {
      applySnapshot(await stopPhoneRemoteIos());
    }
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onWebAction(): Promise<void> {
  if (busy.value) return;
  if (gated.value) {
    emit("navigate", "membership");
    return;
  }
  if (
    web.value.state === "waiting_for_scan" ||
    web.value.state === "waiting_for_approval" ||
    web.value.state === "connected"
  ) {
    showWebModal.value = true;
    return;
  }
  busy.value = true;
  actionError.value = "";
  try {
    applySnapshot(await startPhoneRemoteWeb());
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onApprove(): Promise<void> {
  const request = approval.value;
  if (!request || busy.value) return;
  busy.value = true;
  actionError.value = "";
  try {
    applySnapshot(await approvePhoneRemote(request.requestId));
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onDeny(): Promise<void> {
  const request = approval.value;
  if (!request || busy.value) return;
  busy.value = true;
  actionError.value = "";
  try {
    applySnapshot(await denyPhoneRemote(request.requestId));
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onCancelWaiting(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  actionError.value = "";
  try {
    applySnapshot(await stopPhoneRemoteIos());
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onDisconnectWeb(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  actionError.value = "";
  try {
    applySnapshot(await stopPhoneRemoteWeb());
    showWebModal.value = false;
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onClearTrusted(): Promise<void> {
  if (busy.value) return;
  busy.value = true;
  actionError.value = "";
  try {
    applySnapshot(await clearPhoneRemoteTrustedDevices());
    showClearConfirm.value = false;
  } catch (error) {
    fail(error);
  } finally {
    busy.value = false;
  }
}

async function onOpenAppStore(): Promise<void> {
  try {
    await openIOSAppStore();
  } catch (error) {
    fail(error);
  }
}

async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) throw new Error("clipboard_unavailable");
}

async function onCopyAppStoreLink(): Promise<void> {
  try {
    await copyText(iosAppStoreUrl());
    appStoreCopied.value = true;
    window.setTimeout(() => {
      appStoreCopied.value = false;
    }, 2_000);
  } catch (error) {
    fail(error);
  }
}
</script>

<template>
  <article v-if="visible" class="card mobile-remote-card">
    <div class="mobile-remote-heading">
      <h2>{{ t("无线麦 SayAll 移动端") }}</h2>
    </div>

    <div class="mobile-remote-row" aria-live="polite">
      <span class="mobile-remote-icon ios" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <rect x="7" y="2.8" width="10" height="18.4" rx="2.4" stroke="currentColor" stroke-width="1.5" />
          <path d="M10.4 5.3h3.2M12 18.6h.01" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </span>
      <div class="mobile-remote-copy">
        <h3>{{ t("无线麦 SayAll iOS App") }}</h3>
        <p>{{ t("开启连接后，打开无线麦 SayAll 会优先自动连接，无需扫码。") }}</p>
        <div class="mobile-store-actions">
          <button class="secondary-button compact" type="button" @click="onOpenAppStore">
            {{ t("打开 App Store") }}
          </button>
          <button class="secondary-button compact" type="button" @click="onCopyAppStoreLink">
            {{ appStoreCopied ? t("已复制") : t("复制链接") }}
          </button>
        </div>
      </div>
      <div class="mobile-remote-actions">
        <span class="mobile-status" :class="iosTone">
          <span class="status-dot" :class="iosTone || 'pending'"></span>
          {{ iosStatus }}
        </span>
        <button
          class="secondary-button mobile-action"
          :class="{ 'crown-action': gated }"
          type="button"
          :disabled="busy"
          @click="onIosAction"
        >
          <CrownIcon v-if="gated" class="crown-icon" />
          {{ iosAction }}
        </button>
      </div>
    </div>

    <div v-if="showIosQr" class="mobile-qr-panel">
      <div class="mobile-qr-frame">
        <img :src="ios.qrCode!" :alt="t('iPhone 连接二维码')" />
      </div>
      <div class="mobile-qr-copy">
        <h3>{{ t("备用方式：使用 iOS App 扫码连接") }}</h3>
        <p>{{ t("打开 iOS App 会优先自动连接，无需扫码。如果未找到这台电脑，可在 App 的电脑页面点按“扫描二维码”。") }}</p>
        <p class="small">{{ t("二维码短时有效并会自动更新；停止等待或重新开启后，需要扫描新的二维码。") }}</p>
      </div>
    </div>

    <div class="mobile-remote-row" aria-live="polite">
      <span class="mobile-remote-icon web" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="8.6" stroke="currentColor" stroke-width="1.5" />
          <path
            d="M3.8 12h16.4M12 3.4c2.3 2.4 3.5 5.3 3.5 8.6s-1.2 6.2-3.5 8.6M12 3.4C9.7 5.8 8.5 8.7 8.5 12s1.2 6.2 3.5 8.6"
            stroke="currentColor"
            stroke-width="1.2"
            stroke-linecap="round"
          />
        </svg>
      </span>
      <div class="mobile-remote-copy">
        <h3>{{ t("无线麦 SayAll 微信小程序") }}</h3>
        <p>{{ t("微信扫码即可连接，手机和电脑可使用不同网络。") }}</p>
      </div>
      <div class="mobile-remote-actions">
        <span class="mobile-status" :class="webTone">
          <span class="status-dot" :class="webTone || 'pending'"></span>
          {{ webStatus }}
        </span>
        <button
          class="secondary-button mobile-action"
          :class="{ 'crown-action': gated }"
          type="button"
          :disabled="busy || web.state === 'unavailable'"
          @click="onWebAction"
        >
          <CrownIcon v-if="gated" class="crown-icon" />
          {{ webAction }}
        </button>
      </div>
    </div>

    <p v-if="actionError" class="mobile-error" role="alert">{{ actionError }}</p>

    <div class="mobile-remote-footer">
      <span class="mobile-trust">
        <svg viewBox="0 0 18 18" fill="none" aria-hidden="true">
          <path
            d="M9 2.3 14.2 4v4.1c0 3.1-2.1 5.8-5.2 7.1-3.1-1.3-5.2-4-5.2-7.1V4L9 2.3Z"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linejoin="round"
          />
          <path d="m6.7 8.9 1.5 1.5 3-3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        {{ t("{count} 台受信任设备", { count: trustedCount }) }}
      </span>
      <button
        class="link-button"
        type="button"
        :disabled="trustedCount === 0 || busy"
        @click="showClearConfirm = true"
      >
        {{ t("清除受信任设备") }}
      </button>
    </div>

    <div v-if="approval" class="mobile-modal-layer" role="presentation">
      <div class="mobile-modal" role="dialog" aria-modal="true" aria-labelledby="phone-approval-title">
        <div class="mobile-modal-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <path d="M12 3 19 5.7v5.5c0 4.2-2.8 7.8-7 9.5-4.2-1.7-7-5.3-7-9.5V5.7L12 3Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" />
            <path d="m8.8 11.8 2.1 2.1 4.3-4.3" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </div>
        <h2 id="phone-approval-title">{{ t("允许“{device}”连接？", { device: approval.deviceName }) }}</h2>
        <p>{{ t("这台手机会代替遥控器发送按键和麦克风声音。请确认手机屏幕上的校验码与下方一致。") }}</p>
        <div class="mobile-pairing-label">{{ t("校验码") }}</div>
        <div class="mobile-pairing-code" :aria-label="t('校验码 {code}', { code: approval.pairingCode })">
          {{ approval.pairingCode }}
        </div>
        <p class="mobile-modal-note">{{ t("只允许你认识的设备。允许后，下次连接不再弹出确认。") }}</p>
        <div class="mobile-modal-actions">
          <button class="primary-button" type="button" :disabled="busy" @click="onApprove">
            {{ t("允许连接") }}
          </button>
          <button class="secondary-button" type="button" :disabled="busy" @click="onDeny">
            {{ t("拒绝") }}
          </button>
          <button class="link-button" type="button" :disabled="busy" @click="onCancelWaiting">
            {{ t("取消等待") }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="showWebModal" class="mobile-modal-layer" role="presentation">
      <div class="mobile-modal web" role="dialog" aria-modal="true" aria-labelledby="web-remote-title">
        <button class="mobile-modal-close" type="button" aria-label="关闭" @click="showWebModal = false">
          ×
        </button>
        <div class="mobile-modal-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <circle cx="12" cy="12" r="8.4" stroke="currentColor" stroke-width="1.5" />
            <path
              d="M3.8 12h16.4M12 3.4c2.3 2.4 3.5 5.3 3.5 8.6s-1.2 6.2-3.5 8.6M12 3.4C9.7 5.8 8.5 8.7 8.5 12s1.2 6.2 3.5 8.6"
              stroke="currentColor"
              stroke-width="1.2"
              stroke-linecap="round"
            />
          </svg>
        </div>
        <h2 id="web-remote-title">{{ t("使用微信扫描小程序码") }}</h2>
        <p>{{ t("微信扫码后自动连接，无需输入验证码。手机和电脑可以使用不同网络。") }}</p>
        <div v-if="showWebCode" class="mobile-web-code">
          <img :src="web.miniProgramCode!" :alt="t('微信小程序连接码')" />
        </div>
        <p v-else class="mobile-modal-note">{{ t("小程序码准备中…") }}</p>
        <p class="mobile-modal-note">{{ webStatus }}</p>
        <div class="mobile-modal-actions">
          <button
            v-if="web.state !== 'idle'"
            class="secondary-button"
            type="button"
            :disabled="busy"
            @click="onDisconnectWeb"
          >
            {{ t("断开手机遥控") }}
          </button>
          <button class="link-button" type="button" @click="showWebModal = false">
            {{ t("关闭") }}
          </button>
        </div>
      </div>
    </div>

    <div v-if="showClearConfirm" class="mobile-modal-layer" role="presentation">
      <div class="mobile-modal" role="dialog" aria-modal="true" aria-labelledby="clear-trust-title">
        <h2 id="clear-trust-title">{{ t("清除所有受信任设备？") }}</h2>
        <p>{{ t("当前连接不会中断。下次连接时，需要在电脑上重新允许。") }}</p>
        <div class="mobile-modal-actions">
          <button class="primary-button" type="button" :disabled="busy" @click="onClearTrusted">
            {{ t("清除受信任设备") }}
          </button>
          <button class="secondary-button" type="button" @click="showClearConfirm = false">
            {{ t("取消") }}
          </button>
        </div>
      </div>
    </div>
  </article>
</template>

<style scoped>
.mobile-remote-card {
  margin-top: 14px;
  padding: 15px 16px 0;
}

.mobile-remote-heading h2 {
  margin: 0;
}

.mobile-remote-row {
  display: grid;
  grid-template-columns: 42px minmax(0, 1fr) auto;
  gap: 12px;
  align-items: center;
  padding: 14px 0;
  border-top: 1px solid var(--border);
}

.mobile-remote-heading + .mobile-remote-row {
  margin-top: 10px;
}

.mobile-remote-icon {
  width: 34px;
  height: 34px;
  display: grid;
  place-items: center;
  border-radius: 10px;
  color: var(--accent-text);
  background: var(--accent-surface);
}

.mobile-remote-icon.web {
  color: var(--dot-success);
  background: color-mix(in srgb, var(--dot-success) 12%, transparent);
}

.mobile-remote-icon svg {
  width: 20px;
  height: 20px;
}

.mobile-remote-copy {
  min-width: 0;
}

.mobile-remote-copy h3 {
  margin: 0;
  font-size: 14px;
  line-height: 1.35;
}

.mobile-remote-copy p {
  margin: 4px 0 0;
  color: var(--muted);
  font-size: 12.5px;
  line-height: 1.5;
}

.mobile-store-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.mobile-store-actions .compact {
  min-height: 28px;
  padding: 3px 10px;
  font-size: 12px;
}

.mobile-remote-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 7px;
  min-width: 132px;
}

.mobile-status {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--muted);
  font-size: 12.5px;
  font-weight: 600;
  white-space: nowrap;
}

.mobile-status.warning {
  color: var(--dot-warning);
}

.mobile-status.success {
  color: var(--dot-success);
}

.mobile-status.error {
  color: var(--dot-error);
}

.mobile-status .status-dot {
  width: 8px;
  height: 8px;
}

.mobile-action {
  min-width: 94px;
  min-height: 32px;
  padding: 4px 12px;
  font-size: 12.5px;
}

.mobile-action.crown-action {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
}

.crown-icon {
  width: 14px;
  height: 14px;
}

.mobile-qr-panel {
  display: flex;
  gap: 16px;
  align-items: flex-start;
  padding: 2px 0 16px 54px;
}

.mobile-qr-frame {
  flex: 0 0 auto;
  width: 152px;
  padding: 8px;
  border-radius: 12px;
  background: #fff;
}

.mobile-qr-frame img {
  display: block;
  width: 136px;
  height: 136px;
}

.mobile-qr-copy {
  min-width: 0;
  padding-top: 2px;
}

.mobile-qr-copy h3 {
  margin: 0 0 6px;
  font-size: 13.5px;
}

.mobile-qr-copy p {
  margin: 0 0 6px;
  color: var(--muted);
  font-size: 12.5px;
  line-height: 1.55;
}

.mobile-qr-copy p.small {
  font-size: 12px;
}

.mobile-error {
  margin: 0 0 12px;
  color: var(--dot-error);
  font-size: 12.5px;
}

.mobile-remote-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 11px 0 12px;
  border-top: 1px solid var(--border);
}

.mobile-trust {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  color: var(--muted);
  font-size: 12.5px;
}

.mobile-trust svg {
  width: 16px;
  height: 16px;
}

.mobile-modal-layer {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: grid;
  place-items: center;
  padding: 24px;
  background: rgba(12, 15, 22, 0.42);
  backdrop-filter: blur(2px);
}

.mobile-modal {
  position: relative;
  width: 440px;
  max-width: 100%;
  padding: 22px 24px 18px;
  border: 1px solid var(--border-strong);
  border-radius: 14px;
  background: var(--card);
  box-shadow: 0 26px 70px rgba(8, 11, 18, 0.28);
  text-align: center;
}

.mobile-modal h2 {
  margin: 0 0 7px;
  font-size: 18px;
  line-height: 1.35;
}

.mobile-modal > p {
  margin: 0 auto 8px;
  max-width: 360px;
  color: var(--muted);
  font-size: 13px;
  line-height: 1.6;
}

.mobile-modal-icon {
  width: 44px;
  height: 44px;
  display: grid;
  place-items: center;
  margin: 0 auto 10px;
  border-radius: 50%;
  color: var(--accent-text);
  background: var(--accent-surface);
}

.mobile-modal-icon svg {
  width: 24px;
  height: 24px;
}

.mobile-modal-close {
  position: absolute;
  top: 8px;
  right: 10px;
  width: 28px;
  height: 28px;
  border: 0;
  border-radius: 7px;
  color: var(--muted);
  background: transparent;
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
}

.mobile-modal-close:hover {
  color: var(--text-primary);
  background: var(--surface-subtle);
}

.mobile-pairing-label {
  margin-top: 13px;
  color: var(--muted);
  font-size: 12px;
  font-weight: 600;
}

.mobile-pairing-code {
  margin: 4px 0 8px;
  color: var(--accent-text);
  font-family: "Cascadia Mono", "Consolas", monospace;
  font-size: 42px;
  font-weight: 700;
  letter-spacing: 12px;
  line-height: 1.1;
  text-indent: 12px;
}

.mobile-modal-note {
  margin: 0 auto 13px;
  color: var(--muted);
  font-size: 12px;
  line-height: 1.55;
}

.mobile-web-code {
  width: 180px;
  margin: 12px auto 10px;
  padding: 8px;
  border: 1px solid var(--border);
  border-radius: 14px;
  background: #fff;
}

.mobile-web-code img {
  display: block;
  width: 164px;
  height: 164px;
}

.mobile-modal-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
}

.mobile-modal-actions .link-button {
  min-height: 32px;
  padding: 5px 8px;
}

@media (max-width: 720px) {
  .mobile-remote-row {
    grid-template-columns: 36px minmax(0, 1fr);
  }

  .mobile-remote-actions {
    grid-column: 2;
    align-items: flex-start;
  }

  .mobile-qr-panel {
    padding-left: 0;
  }
}
</style>
