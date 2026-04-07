import { createApp } from "vue";
import FloatApp from "./FloatApp.vue";
import { i18n } from "./i18n";

const app = createApp(FloatApp);
app.use(i18n);
app.mount("#float-app");
