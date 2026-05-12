import { ref } from "vue";

// Module-level singleton: shares dirty state between SettingsPanel and App.vue
const settingsDirty = ref(false);

export function useSettingsStore() {
  return { settingsDirty };
}
