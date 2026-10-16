import { createApp } from "vue";
import App from "./App.vue";
import "./assets/css/main.css";
import { mockIPC } from "@tauri-apps/api/mocks";
import { i18n } from "./i18n";

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
      if (_cmd === "list_tasks" || _cmd === "list_layers") return [];
      if (_cmd === "get_all_settings") return {};
      if (_cmd === "calculate_tile_count") return { total: 0, per_zoom: [] };
      if (_cmd === "plugin:event|listen") return 0;
      if (_cmd === "plugin:event|unlisten") return null;
      return null;
    },
    { shouldMockEvents: true },
  );
}

const app = createApp(App);
// 记录启动或空列表渲染异常，保留组件与调用位置以便定位用户报告。
app.config.errorHandler = (error, _instance, info) => {
  console.error("[App] 界面运行异常", { error, info });
};
app.use(i18n).mount("#app");
