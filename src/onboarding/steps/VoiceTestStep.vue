<script setup lang="ts">
import { computed, ref } from "vue";
import type { VoiceAttemptState } from "../voice-attempt";

const props = defineProps<{
  phase: VoiceAttemptState;
  result: "passed" | "failed" | null;
  failureMessage: string;
  focused: boolean;
  toolLabel: string;
  hotkeyText: string;
  audioText: string;
  /**
   * 核对项（2026-10-13 现场定稿）：有输入法可切（微信 / 豆包）时三项、"输入法"排第一；
   * 其余工具（Vokie / 其他）两项——它们不依赖切换输入法。
   */
  showImeChecklist: boolean;
  /** 输入法兜底提示（2026-10-13 现场）：非空时显示一张说明卡，空串不显示。 */
  imeHint: string;
}>();

const checklist = computed(() => {
  const items: string[] = [];
  if (props.showImeChecklist) {
    items.push(`输入法切到“${props.toolLabel}”。`);
  }
  items.push(`麦克风选 CABLE Output（在“${props.toolLabel}”的语音输入设置里）。`);
  items.push(`工具里的语音键和这次设置一致（${props.hotkeyText}）。`);
  return items;
});

defineEmits<{ input: []; focus: []; blur: []; retry: [] }>();

const box = ref<HTMLInputElement | null>(null);

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

    <div class="onboarding-card warn">
      <h4>开始之前，按你选的工具核对{{ checklist.length === 3 ? "三" : "两" }}件事</h4>
      <ul class="onboarding-numlist">
        <li v-for="(item, index) in checklist" :key="item">
          <span class="onboarding-num">{{ index + 1 }}</span>
          <span>{{ item }}</span>
        </li>
      </ul>
      <p>本次设置快捷键：{{ hotkeyText }}（“本次设置”）；语音设备：{{ audioText }}。</p>
    </div>

    <div v-if="imeHint" class="onboarding-card warn">
      <p>{{ imeHint }}</p>
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

    <p v-if="statusText" class="onboarding-status-line">
      <span class="onboarding-wave-mark" aria-hidden="true">▍▍▍</span>{{ statusText }}
    </p>
    <p v-if="!focused" class="onboarding-muted">先点一下输入框，再按住遥控器语音键。</p>

    <div v-if="result === 'passed'" class="onboarding-card success">
      <p class="onboarding-success">
        <span class="ok-mark" aria-hidden="true">✓</span>
        <span>成功：文字已经出现在输入框里，这一步通过了。</span>
      </p>
    </div>
    <div v-else-if="result === 'failed'" class="onboarding-card warn">
      <p>{{ failureMessage }}</p>
      <div class="onboarding-chips">
        <button class="onboarding-chip strong" type="button" @click="$emit('retry')">
          重新测试
        </button>
      </div>
    </div>
  </section>
</template>
