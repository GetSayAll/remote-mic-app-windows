<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { VoiceAttemptState } from "../voice-attempt";

const props = defineProps<{
  phase: VoiceAttemptState;
  result: "passed" | "failed" | null;
  failureMessage: string;
  focused: boolean;
  toolLabel: string;
  hotkeyText: string;
  audioText: string;
  /** 工具/设备/按键变化时递增，用于清空人工核对项（设计稿 §5.5）。 */
  checklistKey: string;
}>();

defineEmits<{ input: []; focus: []; blur: []; retry: [] }>();

const box = ref<HTMLInputElement | null>(null);
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

function focusBox(): void {
  box.value?.focus();
}

function clearBox(): void {
  if (box.value) {
    box.value.value = "";
  }
}

defineExpose({ focusBox, clearBox });
</script>

<template>
  <section class="onboarding-step">
    <h1>按住遥控器语音键，试一句话</h1>
    <p class="onboarding-lede">
      点一下下面的输入框，按住遥控器上的语音键说一句话，然后松开。文字应该会出现在输入框里。
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

    <input
      ref="box"
      class="onboarding-voice-input"
      type="text"
      placeholder="你说的话会出现在这里"
      autocomplete="off"
      spellcheck="false"
      @input="$emit('input')"
      @focus="$emit('focus')"
      @blur="$emit('blur')"
    />

    <p v-if="statusText" class="onboarding-status-line">{{ statusText }}</p>
    <p v-if="!focused" class="onboarding-muted">先点一下输入框，再按住遥控器语音键。</p>

    <div v-if="result === 'passed'" class="onboarding-callout ok">
      <p class="onboarding-ok">成功：文字已经出现在输入框里，这一步通过了。</p>
    </div>
    <div v-else-if="result === 'failed'" class="onboarding-callout warn">
      <p>{{ failureMessage }}</p>
      <div class="onboarding-actions">
        <button class="secondary-button" type="button" @click="$emit('retry')">重新测试</button>
      </div>
    </div>
  </section>
</template>
