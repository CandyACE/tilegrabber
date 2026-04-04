import { createApp } from "vue";
import App from "./App.vue";
import "./assets/css/main.css";
import { mockIPC } from "@tauri-apps/api/mocks";

window.addEventListener(
  "contextmenu",
  (event) => {
    event.preventDefault();
  },
  { capture: true },
);

// 在浏览器（非 Tauri）环境中 mock Tauri IPC，防止 invoke/listen 报错
if (!("__TAURI_INTERNALS__" in window)) {
  mockIPC(
    (_cmd, _payload) => {
      if (_cmd === "list_tasks") return [];
      if (_cmd === "calculate_tile_count") return { total: 0, per_zoom: [] };
      if (_cmd === "plugin:event|listen") return 0;
      if (_cmd === "plugin:event|unlisten") return null;
      return null;
    },
    { shouldMockEvents: true },
  );
}

createApp(App).mount("#app");
