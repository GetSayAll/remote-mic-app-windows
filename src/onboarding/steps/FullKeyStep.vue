<script setup lang="ts">
import { ref, watch } from "vue";
import { CAPTURE_MORE_INPUT_TOOLS_NOTE } from "../../lib/capture-switch";
import { t } from "../../lib/i18n";

/**
 * 第④步「全按键支持」说明与引导（2026-11-05 用户要求；位于语音设备之后）。
 *
 * 这一步只做两件事：说清开关的作用与代价，并给出开启入口。全按键支持是可选增强
 * ——基础语音与普通按键不依赖系统授权，所以开关状态不参与「继续」判定（ARM64 等
 * 本机不可用场景也必须能走完向导）。文案规范见 docs/product-copy.md。
 */
const props = defineProps<{
  /** 开关设置（用户意图）。 */
  enabled: boolean;
  /** 本机能否使用该功能（ARM64 等不可用时置灰开关）。 */
  available: boolean;
  observeOnly?: boolean;
  /** 开启/关闭请求进行中。 */
  busy: boolean;
  /** 首次读取状态中：开关先置灰，避免把"还没读到"显示成"关着"。 */
  loading: boolean;
  /** 开关旁唯一的失败/状态文字（原文只进日志，界面只显示映射后的用户语言）。 */
  failureText: string | null;
}>();

const emit = defineEmits<{ toggle: [] }>();

/**
 * 原生复选框被点击时浏览器先翻 checked；父组件在"确认授权"阶段可能不改状态，
 * Vue 不会生成 DOM 补丁，会出现「状态是关、界面是开」。这里先写回绑定值，
 * 等父组件完成后由 watch 同步（与输入工具页/按键页同款防御）。
 */
const switchEl = ref<HTMLInputElement | null>(null);

function onChange(): void {
  if (switchEl.value) {
    switchEl.value.checked = props.enabled;
  }
  emit("toggle");
}

watch(
  () => props.enabled,
  (value) => {
    if (switchEl.value) switchEl.value.checked = value;
  },
);
</script>

<template>
  <section v-if="observeOnly" class="onboarding-step">
    <h1>{{ t("全按键支持") }}</h1>
    <p class="onboarding-lede">{{ t("当前遥控器的普通按键仅供查看。你可以继续设置听写。") }}</p>
  </section>
  <section v-else class="onboarding-step">
    <h1>{{ t("开启全按键支持") }}</h1>
    <p class="onboarding-lede">
      {{
        t(
          "开启后，遥控器上的所有按键都能设置自定义功能，包括 Windows 默认收不到的返回 / 音量+ / 音量−。",
        )
      }}
    </p>

    <!-- 建议开启的强调提示（2026-11-05 用户要求）：让用户在可选步骤里也能一眼看到
         为什么值得开启；开关本身仍可跳过。 -->
    <div class="onboarding-recommend" role="note">
      <span class="onboarding-recommend-tag">{{ t("建议开启") }}</span>
      <span>{{ t("开启后，返回 / 音量+ / 音量− 才能自定义；豆包语音输入也需要它。") }}</span>
    </div>

    <div class="onboarding-card">
      <div class="onboarding-switch-row onboarding-switch-row--first">
        <label class="onboarding-switch">
          <span>{{ t("全按键支持") }}</span>
          <span class="toggle-switch-slot">
            <input
              ref="switchEl"
              type="checkbox"
              class="toggle-input"
              :checked="enabled"
              :disabled="loading || busy || !available"
              @change="onChange"
            />
            <span v-if="busy" class="toggle-spinner" aria-hidden="true"></span>
          </span>
        </label>
      </div>
      <!-- 开关旁只允许一句状态文案：失败原因优先，其次是本机不可用/已开启/可跳过。
           操作进行中（关闭 / 开启 / 授权）不显示任何文字，只有开关上的转圈。 -->
      <p v-if="!busy && failureText" class="onboarding-capture-failure">{{ failureText }}</p>
      <p v-else-if="!busy && !available" class="onboarding-capture-note">
        {{ t("这台电脑暂不支持全按键支持，其他按键不受影响。") }}
      </p>
      <p v-else-if="!busy && enabled" class="onboarding-capture-note onboarding-capture-ok">
        {{ t("已开启。返回 / 音量+ / 音量− 现在也能自定义。") }}
      </p>
      <p v-else-if="!busy && !loading" class="onboarding-capture-note">
        {{ t("现在不开启也可以继续；以后可以在“按键”页随时打开。") }}
      </p>
    </div>

    <ul class="onboarding-list">
      <li>{{ t(CAPTURE_MORE_INPUT_TOOLS_NOTE) }}</li>
      <li>{{ t("开启时 Windows 会弹出授权窗口，请选择“是”。") }}</li>
      <li>{{ t("只读取这支遥控器的按键，不收集其他内容。") }}</li>
      <li>
        {{ t("个别游戏带有防作弊保护，玩这类游戏前建议先关闭全按键支持。") }}
      </li>
    </ul>
  </section>
</template>
