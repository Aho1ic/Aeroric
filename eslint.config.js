import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactPlugin from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import prettier from "eslint-config-prettier";

export default tseslint.config(
  // 忽略构建产物与生成文件
  {
    ignores: ["dist/**", "src-tauri/**", "node_modules/**", "*.config.js"],
  },

  // JavaScript 基础规则
  js.configs.recommended,

  // TypeScript 推荐规则（不启用类型感知规则，避免过度配置）
  ...tseslint.configs.recommended,

  // React 规则
  {
    plugins: {
      react: reactPlugin,
      "react-hooks": reactHooks,
    },
    settings: {
      react: { version: "detect" },
    },
    rules: {
      // React Hooks 规则：违反会导致运行时 bug，必须开启
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",

      // React 17+ JSX transform，无需手动 import React
      "react/react-in-jsx-scope": "off",
      "react/prop-types": "off",

      // TypeScript 调整：已有代码存在少量 any，初期用 warn 而非 error
      "@typescript-eslint/no-explicit-any": "warn",
      // 允许未使用变量以 _ 开头的命名惯例
      "@typescript-eslint/no-unused-vars": [
        "warn",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],

      // 禁止 console.log 遗留（warn 级别，生产前清理）
      "no-console": ["warn", { allow: ["warn", "error"] }],
    },
  },

  // 禁止从 plugin-dialog 引入提示类弹窗。
  //
  // 背景：plugin-dialog 的 confirm / message / ask 走 OS 原生 MessageBox，
  // 在 Windows 上就是系统提示框——配色不随主题、标题栏是系统的、拿不到设计
  // token，与应用其余部分割裂。已统一改走 src/lib/appDialog 的应用内弹窗。
  //
  // open / save 不在禁用名单里：那是 OS 文件选择器，无法用应用内 UI 复刻，
  // 也不是「提示弹窗」，继续用原生是有意的。
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/lib/appDialog.ts"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          paths: [
            {
              name: "@tauri-apps/plugin-dialog",
              importNames: ["confirm", "message", "ask"],
              message:
                "提示类弹窗请用 src/lib/appDialog 的 confirm（应用内弹窗）。plugin-dialog 的 confirm/message/ask 会弹 OS 系统框。open / save 不受限制。",
            },
          ],
        },
      ],

      // 同理禁掉浏览器自带的 alert / confirm / prompt。WebView2 把它们渲染成
      // 系统框，和 plugin-dialog 的原生 MessageBox 是同一个问题。
      //
      // 这条规则是补的：第一轮清理漏了 4 处 `window\n  .prompt(...)`，跨行写法
      // 单行 grep 抓不到，只有 lint 能稳定兜住。
      "no-restricted-properties": [
        "error",
        {
          object: "window",
          property: "prompt",
          message: "请用 src/lib/appDialog 的 prompt（应用内输入框，返回 Promise）。",
        },
        {
          object: "window",
          property: "confirm",
          message: "请用 src/lib/appDialog 的 confirm（应用内确认框）。",
        },
        {
          object: "window",
          property: "alert",
          message: "请用应用内提示（toast 或 appDialog），不要弹系统框。",
        },
      ],
      "no-restricted-globals": [
        "error",
        {
          name: "prompt",
          message: "请用 src/lib/appDialog 的 prompt（应用内输入框，返回 Promise）。",
        },
        {
          name: "confirm",
          message: "请用 src/lib/appDialog 的 confirm（应用内确认框）。",
        },
        {
          name: "alert",
          message: "请用应用内提示（toast 或 appDialog），不要弹系统框。",
        },
      ],
    },
  },

  // 关闭与 Prettier 冲突的格式化规则（必须放在最后）
  prettier,
);
