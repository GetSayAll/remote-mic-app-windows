<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import Sidebar from "./components/Sidebar.vue";
import { getRuntimeSnapshot, type RuntimeSnapshot } from "./lib/bridge";
import type { PageId } from "./navigation";
import AboutPage from "./pages/AboutPage.vue";
import ButtonsPage from "./pages/ButtonsPage.vue";
import ConnectionPage from "./pages/ConnectionPage.vue";
import PermissionsPage from "./pages/PermissionsPage.vue";
import StatisticsPage from "./pages/StatisticsPage.vue";

const activePage = ref<PageId>("connection");
const runtime = ref<RuntimeSnapshot | null>(null);
const loadError = ref("");

const activeComponent = computed(() => ({
  buttons: ButtonsPage,
  statistics: StatisticsPage,
  connection: ConnectionPage,
  permissions: PermissionsPage,
  about: AboutPage,
})[activePage.value]);

onMounted(async () => {
  try {
    runtime.value = await getRuntimeSnapshot();
  } catch (error) {
    loadError.value = error instanceof Error ? error.message : String(error);
  }
});
</script>

<template>
  <div class="app-shell">
    <Sidebar :active-page="activePage" @select="activePage = $event" />
    <main class="content">
      <div v-if="loadError" class="error-banner">无法读取运行状态：{{ loadError }}</div>
      <component :is="activeComponent" :runtime="runtime" />
    </main>
  </div>
</template>
