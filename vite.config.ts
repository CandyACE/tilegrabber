import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { resolve } from "path";

export default defineConfig({
  plugins: [vue(), tailwindcss()],

  resolve: {
    alias: {
      "~": resolve(__dirname, "./src"),
      "@": resolve(__dirname, "./src"),
    },
  },

  // Vite 为 Tauri 优化
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],

  server: {
    port: 4000,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },

  build: {
    outDir: "dist",
    rollupOptions: {
      input: {
        main: resolve(__dirname, "index.html"),
        float: resolve(__dirname, "float.html"),
      },
      output: {
        manualChunks: (id) => {
          if (id.includes("maplibre-gl")) return "maplibre";
        },
      },
    },
  },
});
