import { describe, expect, it } from "vitest";

import {
  remoteTerminalLayerStyle,
  shellCenterLayerStyle,
} from "../components/project-page/viewMode";

describe("remoteTerminalLayerStyle", () => {
  it("可见时铺满中央工作区", () => {
    expect(remoteTerminalLayerStyle(true)).toEqual({
      position: "absolute",
      inset: 0,
      display: "flex",
      zIndex: 4,
    });
  });

  it("不可见时既不占位也不参与层叠", () => {
    // `display: none` 和 `zIndex: 0` 要同时成立。只改 display 的话,终端还在层叠上下文里,
    // 后续有半透明覆盖层时会命中它;只改 zIndex 的话它仍然吃点击。
    expect(remoteTerminalLayerStyle(false)).toEqual({
      position: "absolute",
      inset: 0,
      display: "none",
      zIndex: 0,
    });
  });

  describe("和本地 shell 覆盖层的关系", () => {
    // 下面两条是**防合并的闸门**。SSH 和 WSL 两块的包裹 div 原先逐字节相同,合成了
    // `remoteTerminalLayerStyle`;但**不能再往前一步和 `shellCenterLayerStyle` 合并** ——
    // 谁想省掉一个函数会先看到这两条挂掉。

    it("远端压在本地 shell 上面", () => {
      // 关键在于 WSL / SSH 项目下本地 shell 那块也可能挂着:它的挂载条件
      // (`shellTerminalMounted && projectLocation.kind !== "ssh"`)只排除了 ssh,
      // WSL 项目完全满足。两层同时可见时靠这一级 z-index 差决定谁在上面,
      // 合并成同一个值会让 WSL 终端被本地 shell 盖住。
      const remote = remoteTerminalLayerStyle(true).zIndex as number;
      const local = shellCenterLayerStyle(true).zIndex as number;

      expect(remote).toBeGreaterThan(local);
    });

    it("本地那份额外带 flex 收缩相关的属性", () => {
      // 本地 shell 的内容用 `shellCenterContentStyle` 再包一层,需要这三个属性
      // 才不会被子内容撑开;远端面板自己管尺寸,没有这一层。
      const local = shellCenterLayerStyle(true);

      expect(local).toMatchObject({ minWidth: 0, minHeight: 0, alignItems: "stretch" });
      expect(remoteTerminalLayerStyle(true)).not.toHaveProperty("alignItems");
    });
  });
});
