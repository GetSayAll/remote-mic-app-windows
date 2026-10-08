<script setup lang="ts">
import { ref, watch } from "vue";
import type { KeyCode, VoiceInputTool, VokieInstallation } from "../../lib/bridge";
import OptionCard from "../../components/onboarding/OptionCard.vue";

const props = defineProps<{
  tool: VoiceInputTool | null;
  hotkeyLabel: string;
  captureEnabled: boolean;
  captureBusy: boolean;
  captureHint: string;
  conflict: boolean;
  vokie: VokieInstallation | null;
  vokieBusy: boolean;
  otherKeys: KeyCode[] | null;
  message: string;
}>();

const emit = defineEmits<{
  selectTool: [tool: VoiceInputTool];
  toggleCapture: [];
  openVokieSite: [];
  launchVokie: [];
  refreshVokie: [];
  chooseOtherKeys: [keys: KeyCode[]];
}>();

/** 卡片顺序固定：豆包排第一（与连接页一致的推荐顺序）。 */
const TOOL_CARDS: Array<{ id: VoiceInputTool; title: string; blurb: string }> = [
  { id: "doubao", title: "豆包输入法", blurb: "按住遥控器语音键 = 右 Alt" },
  { id: "wechat", title: "微信输入法", blurb: "按住遥控器语音键 = 左 Ctrl + 左 Win" },
  { id: "vokie", title: "Vokie", blurb: "按住遥控器语音键 = 右 Alt" },
  { id: "other", title: "其他工具", blurb: "自己选一个语音键" },
];

const OTHER_OPTIONS: Array<{ label: string; keys: KeyCode[] }> = [
  { label: "右 Alt", keys: ["right_alt"] },
  { label: "左 Alt", keys: ["left_alt"] },
  { label: "不按键", keys: [] },
];

/**
 * 「支持更多输入工具」开关的原生复选框：被点击的瞬间浏览器先翻 checked，
 * 而"每次开启都要先确认授权"期间状态未变，Vue 不会生成 DOM 补丁——会出现
 * 「状态是关、界面是开」。这里在 change 里先写回 props 的真值，等父组件完成
 * 后由 watch 同步（与连接页/按键页同款防御）。
 */
const captureSwitchEl = ref<HTMLInputElement | null>(null);

function onCaptureChange(): void {
  if (captureSwitchEl.value) {
    captureSwitchEl.value.checked = props.captureEnabled;
  }
  emit("toggleCapture");
}

watch(
  () => props.captureEnabled,
  (value) => {
    if (captureSwitchEl.value) captureSwitchEl.value.checked = value;
  },
);

function isOtherKeySelected(keys: KeyCode[]): boolean {
  return JSON.stringify(props.otherKeys ?? null) === JSON.stringify(keys);
}
</script>

<template>
  <section class="onboarding-step">
    <h1>选择你要用的输入工具</h1>
    <p class="onboarding-lede">
      无线麦会替你按住它的语音键；选好后，下面显示这个工具还需要怎么设置。
    </p>

    <div v-if="conflict" class="onboarding-card warn">
      <h4>检测到 Vokie 正在运行</h4>
      <p>它会抢先响应右 Alt，豆包收不到语音键。请先退出 Vokie，再点「重新检测」。</p>
      <div class="onboarding-chips">
        <button class="onboarding-chip strong" type="button" @click="$emit('refreshVokie')">
          重新检测
        </button>
      </div>
    </div>

    <div class="onboarding-grid2">
      <OptionCard
        v-for="card in TOOL_CARDS"
        :key="card.id"
        :selected="tool === card.id"
        @select="$emit('selectTool', card.id)"
      >
        <strong>{{ card.title }}</strong>
        <small>{{ card.blurb }}</small>
      </OptionCard>
    </div>

    <div v-if="tool === 'doubao'" class="onboarding-card">
      <h4>按住遥控器语音键 = {{ hotkeyLabel }}<span class="onboarding-tag">本次设置</span></h4>
      <p>在豆包里把麦克风选为 CABLE Output；长按语音键保持右 Alt（出厂默认就是它）。</p>
      <div class="onboarding-switch-row">
        <label class="onboarding-switch">
          <span>支持更多输入工具</span>
          <input
            ref="captureSwitchEl"
            type="checkbox"
            class="toggle-input"
            :checked="captureEnabled"
            :disabled="captureBusy"
            @change="onCaptureChange"
          />
        </label>
        <span class="switch-state" :class="captureEnabled ? 'ok' : 'warn'">
          {{ captureBusy ? "正在设置…" : captureEnabled ? "已开启" : "需要开启" }}
        </span>
      </div>
      <p v-if="captureHint" class="onboarding-error">{{ captureHint }}</p>
      <p class="onboarding-muted">每次开启都会弹出 Windows 授权窗口，请选择“是”。</p>
    </div>

    <div v-else-if="tool === 'wechat'" class="onboarding-card">
      <h4>按住遥控器语音键 = 左 Ctrl + 左 Win<span class="onboarding-tag">本次设置</span></h4>
      <p>在微信输入法里把麦克风选为 CABLE Output；语音需要联网，按住约半秒以上再说话。</p>
    </div>

    <div v-else-if="tool === 'vokie'" class="onboarding-card">
      <h4>按住遥控器语音键 = {{ hotkeyLabel }}<span class="onboarding-tag">本次设置</span></h4>
      <template v-if="vokie && vokie.running">
        <p class="onboarding-ok">Vokie 正在运行，这一项可以继续了。</p>
      </template>
      <template v-else-if="vokie && vokie.installed">
        <p>Vokie 已安装但没有运行：先启动它，再回来「重新检测」。</p>
        <div class="onboarding-chips">
          <button
            class="onboarding-chip strong"
            type="button"
            :disabled="vokieBusy"
            @click="$emit('launchVokie')"
          >
            {{ vokieBusy ? "正在启动…" : "打开 Vokie" }}
          </button>
          <button class="onboarding-chip" type="button" @click="$emit('refreshVokie')">
            重新检测
          </button>
        </div>
      </template>
      <template v-else>
        <p>没有检测到 Vokie。先安装，再回来「重新检测」。</p>
        <div class="onboarding-chips">
          <button class="onboarding-chip strong" type="button" @click="$emit('openVokieSite')">
            打开官网 vokie.com
          </button>
          <button class="onboarding-chip" type="button" @click="$emit('refreshVokie')">
            重新检测
          </button>
        </div>
      </template>
      <p class="onboarding-muted">在 Vokie 里把麦克风选为 CABLE Output，快捷键保持右 Alt。</p>
    </div>

    <div v-else-if="tool === 'other'" class="onboarding-card">
      <h4>按住遥控器语音键 = {{ hotkeyLabel }}</h4>
      <p>选与该工具里一致的语音键；选「不按键」只把声音送到 CABLE，不替它按键：</p>
      <div class="onboarding-chip-select">
        <button
          v-for="option in OTHER_OPTIONS"
          :key="option.label"
          type="button"
          :class="{ selected: isOtherKeySelected(option.keys) }"
          @click="$emit('chooseOtherKeys', option.keys)"
        >
          {{ option.label }}
        </button>
      </div>
      <p class="onboarding-muted">工具里把麦克风选为 CABLE Output。</p>
    </div>

    <p v-if="message" class="onboarding-muted">{{ message }}</p>
  </section>
</template>
