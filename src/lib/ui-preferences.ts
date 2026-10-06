import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

export interface UiPreferences {
  lockButtonSelection: boolean;
  templatesExpanded: boolean;
  associationsExpanded: boolean;
}

// Presentation preferences only; never reapplies mappings or changes device state.
export function useUiPreference(field: keyof UiPreferences) {
  const value = ref(true);
  const loaded = ref(false);
  const pending = ref(false);
  const error = ref<string | null>(null);
  onMounted(async () => {
    try {
      const saved = await invoke<UiPreferences>("get_ui_preferences");
      value.value = saved[field];
      loaded.value = true;
    } catch { error.value = "未能读取界面偏好，未更改已保存设置。"; }
  });
  async function save(enabled: boolean) {
    if (!loaded.value || pending.value || enabled === value.value) return;
    pending.value = true; error.value = null;
    try {
      await invoke("set_ui_preference", { field, enabled });
      value.value = enabled;
    } catch { error.value = "界面偏好保存失败，已保留上次保存的值。"; }
    finally { pending.value = false; }
  }
  function toggleDetails(event: Event) {
    const element = event.target as HTMLDetailsElement;
    const enabled = element.open;
    // Mount/loading and programmatic open changes must never write defaults back.
    element.open = value.value;
    void save(enabled);
  }
  function toggleCheckbox(event: Event) {
    const element = event.target as HTMLInputElement;
    const enabled = element.checked;
    element.checked = value.value;
    void save(enabled);
  }
  return { value, loaded, pending, error, toggleDetails, toggleCheckbox };
}
