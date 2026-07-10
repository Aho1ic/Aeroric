import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    testTimeout: 15000,
    // Node 25 exposes an experimental global localStorage without a persistence path,
    // which emits one warning per Vitest worker. Tests use jsdom's isolated storage.
    execArgv: ["--no-experimental-webstorage"],
    // jsdom 模拟浏览器环境，支持 DOM API 和 localStorage
    environment: "jsdom",
    // 全局注入 expect、describe、it 等，无需每个文件手动 import
    globals: true,
    // 在每个测试文件运行前执行的 setup（引入 @testing-library/jest-dom 扩展）
    setupFiles: ["./src/test/setup.ts"],
    // 覆盖率报告
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/test/**", "src/vite-env.d.ts", "src/styles.ts"],
    },
  },
});
