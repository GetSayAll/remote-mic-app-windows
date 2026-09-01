<script setup lang="ts">
import type { PageId } from "../navigation";
import { navigationItems } from "../navigation";

defineProps<{ activePage: PageId }>();
const emit = defineEmits<{ select: [page: PageId] }>();
</script>

<template>
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-mark">S</div>
      <div>
        <strong>无线麦</strong>
        <span>SayAll for Windows</span>
      </div>
    </div>

    <nav aria-label="设置页面">
      <button
        v-for="item in navigationItems"
        :key="item.id"
        class="nav-item"
        :class="{ active: activePage === item.id }"
        type="button"
        @click="emit('select', item.id)"
      >
        <span class="nav-icon" aria-hidden="true">{{ item.icon }}</span>
        <span>{{ item.label }}</span>
      </button>
    </nav>

    <div class="sidebar-footer">
      <span class="status-dot pending"></span>
      Windows Preview
    </div>
  </aside>
</template>
