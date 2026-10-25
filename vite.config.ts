import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 在移动端/桌面端会注入 TAURI_ 前缀的环境变量
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: host || "0.0.0.0",
    port: 5173,
    strictPort: true,
    allowedHosts: true,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // Windows 端使用 WebView2（Chromium），可直接使用较新的语法
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks: {
          react: ["react", "react-dom"],
          charts: ["recharts"],
        },
      },
    },
  },
});
