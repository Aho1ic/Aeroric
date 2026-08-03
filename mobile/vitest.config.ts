import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    testTimeout: 15000,
    hookTimeout: 15000,
    include: ["src/**/*.test.ts", "scripts/**/*.test.mjs"],
  },
});
