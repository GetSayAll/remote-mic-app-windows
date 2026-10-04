<script setup lang="ts">
import { computed } from "vue";
import {
  isRecommendedVoiceEndpoint,
  type AudioEndpoint,
  type AudioSnapshot,
} from "../../lib/bridge";

const props = defineProps<{
  endpoints: AudioEndpoint[];
  audio: AudioSnapshot | null;
  scanning: boolean;
  message: string;
  selectingEndpointId: string;
  openingVbCablePage: boolean;
}>();

defineEmits<{
  select: [endpoint: AudioEndpoint];
  refresh: [];
  openDownload: [];
}>();

const recommended = computed(() => props.endpoints.filter(isRecommendedVoiceEndpoint));
const selectedId = computed(() => props.audio?.selectedEndpointId ?? null);
const ready = computed(() => props.audio?.phase === "ready");
</script>

<template>
  <section class="onboarding-step">
    <h1>选择语音设备</h1>
    <p class="onboarding-lede">
      无线麦把遥控器的声音送到播放端（CABLE Input）；语音工具再从录音端（CABLE Output）读取声音。
    </p>

    <div v-if="recommended.length === 0" class="onboarding-callout warn">
      <p>这台电脑还没有 VB-CABLE 语音设备。先安装，再继续：</p>
      <ol>
        <li>下载压缩包并完整解压，右键安装程序选择「以管理员身份运行」。</li>
        <li>安装完成后重启电脑，再回来点「重新检测」。</li>
      </ol>
      <div class="onboarding-actions">
        <button
          class="primary-button"
          type="button"
          :disabled="openingVbCablePage"
          @click="$emit('openDownload')"
        >
          {{ openingVbCablePage ? "正在打开…" : "打开官方下载页" }}
        </button>
        <button class="secondary-button" type="button" :disabled="scanning" @click="$emit('refresh')">
          {{ scanning ? "正在检测…" : "重新检测" }}
        </button>
      </div>
    </div>

    <div v-else class="onboarding-callout">
      <p>选择带「推荐」标记的设备：</p>
      <ul class="onboarding-device-list">
        <li v-for="endpoint in recommended" :key="endpoint.id">
          <span>
            {{ endpoint.name }}
            <em class="onboarding-badge">推荐</em>
          </span>
          <button
            class="secondary-button"
            type="button"
            :disabled="selectingEndpointId === endpoint.id || (selectedId === endpoint.id && ready)"
            @click="$emit('select', endpoint)"
          >
            <template v-if="selectedId === endpoint.id && ready">已就绪</template>
            <template v-else-if="selectingEndpointId === endpoint.id">正在打开…</template>
            <template v-else>选择</template>
          </button>
        </li>
      </ul>
    </div>

    <p v-if="message" class="onboarding-message">{{ message }}</p>
  </section>
</template>
