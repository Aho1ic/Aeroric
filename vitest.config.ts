import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    // 覆盖率插桩 + 并行 worker 会让重 UI 测试的墙钟时间放大 5~6 倍(CPU 超订),
    // 15s 对最慢的几个 DBX 网格测试只剩 ~20% 余量,在更慢的 CI runner 上会翻转成超时。
    // 全局给到 30s,个别已知偏重的测试再单独放宽。
    testTimeout: 30000,
    // Node 25 exposes an experimental global localStorage without a persistence path,
    // which emits one warning per Vitest worker. Tests use jsdom's isolated storage.
    execArgv: ["--no-experimental-webstorage"],
    // jsdom 模拟浏览器环境，支持 DOM API 和 localStorage
    environment: "jsdom",
    // 全局注入 expect、describe、it 等，无需每个文件手动 import
    globals: true,
    // 移动端有自己的 Vitest 配置和 CI 任务，避免根配置重复收集 mobile 测试。
    include: ["src/**/*.test.{ts,tsx}"],
    // 在每个测试文件运行前执行的 setup（引入 @testing-library/jest-dom 扩展）
    setupFiles: ["./src/test/setup.ts"],
    // 覆盖率报告
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/test/**", "src/vite-env.d.ts", "src/styles.ts"],
      // 全局门槛按**实测值往下留 ~1 个点**的余量,起棘轮作用:
      // 2026-08-31 实测 74.42 / 69.28 / 71.73 / 77.28(311 文件 4326 测试全绿,
      // 排掉 notebook-panel.test.tsx 等禁区测试文件。测试文件本身不属于生产
      // 代码，按设计不纳入覆盖率统计；禁区测试因交互型与真实文件操作特性，
      // 刻意从统计中排除，避免实时时钟等环境分支干扰门槛。其行为仍由对应的
      // 专项测试套件（notebook-panel-* 系列）完整覆盖，统计盲区属有意取舍。
      // 留余量是因为个别用例带真实时钟分支,卡死在实测值会让门槛偶发翻红;
      // 1 个点足够挡住成片的回归。
      // 覆盖率再上一台阶时同步抬这四个数,不要让它长期低于实测值。
      thresholds: {
        statements: 73,
        branches: 68,
        functions: 70,
        lines: 76,
        "src/appRemoteEvents.ts": {
          statements: 100,
          branches: 100,
          functions: 100,
          lines: 100,
        },
      },
    },
  },
});
