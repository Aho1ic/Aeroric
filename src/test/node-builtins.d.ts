/**
 * 测试用的最小 Node 内建声明。
 *
 * 项目没有装 `@types/node`(前端源码不需要),但少数测试要从磁盘读源文件做静态
 * 断言(如 `z-layers.test.ts` 校验 CSS 与 zLayers.ts 一致)。这里只声明用到的
 * 那几个 API,避免为此引入整套 Node 类型。
 */

declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}

declare module "node:path" {
  export function resolve(...segments: string[]): string;
}

declare const process: { cwd(): string };
