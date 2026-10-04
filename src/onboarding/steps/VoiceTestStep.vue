<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { VoiceAttemptState } from "../voice-attempt";

const props = defineProps<{
  phase: VoiceAttemptState;
  result: "passed" | "failed" | null;
  failureMessage: string;
  /** 原生测试输入框是否打开（200ms 轮询）。 */
  boxOpen: boolean;
  /** 原生测试输入框是否处于前台焦点。 */
  focused: boolean;
  toolLabel: string;
  hotkeyText: string;
  audioText: string;
  /** 工具/设备/按键变化时递增，用于清空人工核对项（设计稿 §5.5）。 */
  checklistKey: string;
}>();

defineEmits<{ retry: [] }>();

const micChecked = ref(false);
const hotkeyChecked = ref(false);

watch(
  () => props.checklistKey,
  () => {
    micChecked.value = false;
    hotkeyChecked.value = false;
  },
);

const statusText = computed(() => {
  switch (props.phase) {
    case "streaming":
      return "正在接收你说的话…";
    case "waiting_end":
      return "正在收尾…";
    case "waiting_transcript":
      return "正在等文字出现…";
    case "terminal":
      return "";
    default:
      return "等待你按住遥控器语音键。";
  }
});
</script>

<template>
  <section class="onboarding-step">
    <h1>按住遥控器语音键，试一句话</h1>
    <p class="onboarding-lede">
      测试输入框已经单独弹出（标题「语音测试输入框」）。点一下它，按住遥控器上的语音键说一句话，
      然后松开。文字应该会出现在那个框里。
    </p>

    <div class="onboarding-callout">
      <p class="onboarding-status-line">开始之前，按你选的工具核对两件事</p>
      <ul class="onboarding-check-list">
        <li>麦克风选 CABLE Output（在“{{ toolLabel }}”的声音设置里）</li>
        <li>工具里的语音键和这次设置一致（{{ hotkeyText }}）</li>
      </ul>
      <label class="onboarding-check-row">
        <input v-model="micChecked" type="checkbox" />
        <span>工具的麦克风已选 CABLE Output</span>
      </label>
      <label class="onboarding-check-row">
        <input v-model="hotkeyChecked" type="checkbox" />
        <span>工具里的语音键与“{{ hotkeyText }}”一致</span>
      </label>
      <p class="onboarding-muted">
        本次设置快捷键：{{ hotkeyText }}（“本次设置”）；语音设备：{{ audioText }}。
      </p>
    </div>

    <p v-if="!boxOpen" class="onboarding-muted">
      测试输入框没有打开。点下面的按钮重新打开它。
    </p>
    <p v-else-if="!focused" class="onboarding-muted">
      先点一下弹出的「语音测试输入框」，再按住遥控器语音键。
    </p>
    <p v-if="statusText" class="onboarding-status-line">{{ statusText }}</p>

    <div v-if="!boxOpen && result === null" class="onboarding-actions">
      <button class="secondary-button" type="button" @click="$emit('retry')">
        打开测试输入框
      </button>
    </div>

    <div v-if="result === 'passed'" class="onboarding-callout ok">
      <p class="onboarding-ok">成功：文字已经出现在测试输入框里，这一步通过了。</p>
    </div>
    <div v-else-if="result === 'failed'" class="onboarding-callout warn">
      <p>{{ failureMessage }}</p>
      <div class="onboarding-actions">
        <button class="secondary-button" type="button" @click="$emit('retry')">重新测试</button>
      </div>
    </div>
  </section>
</template>
