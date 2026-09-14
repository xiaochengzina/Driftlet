import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: {
      // 编辑器原子写（临时目录 + rename）会在源码目录留下 .*.tmpdir/*.tmp
      // 瞬时文件，chokidar 对它们 watch 即 EBUSY 崩溃（实机踩过）——排除之
      ignored: ["**/src-tauri/**", "**/.*.tmpdir/**", "**/*.tmp"],
    },
  },
  build: {
    rollupOptions: {
      input: {
        // 日志窗口是第二个独立页面（log.html?theme=…&lang=… 由后端烘焙参数）
        main: fileURLToPath(new URL("index.html", import.meta.url)),
        log: fileURLToPath(new URL("log.html", import.meta.url)),
      },
    },
  },
}));
