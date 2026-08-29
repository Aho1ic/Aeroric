/* 附件地址解析层。
 *
 * 这里的每一条都对应一个"图会静默显示不出来"或者"链接指到 vault 外面"的坑,
 * 而这两类问题在 UI 上都表现为一个空白框 —— 没有测试的话只能靠肉眼发现。
 */

import { describe, expect, it } from "vitest";
import {
  attachmentMarkdown,
  joinNotePath,
  linkFromNote,
  needsVaultResolve,
  noteDirOf,
  vaultRelativePath,
} from "../components/notebook/attachmentUrls";

describe("needsVaultResolve", () => {
  it("只有相对路径要解析", () => {
    expect(needsVaultResolve("attachments/x.png")).toBe(true);
    expect(needsVaultResolve("../attachments/x.png")).toBe(true);
    expect(needsVaultResolve("./x.png")).toBe(true);
  });

  it("带 scheme 的一律不解析", () => {
    for (const url of [
      "https://example.com/x.png",
      "http://example.com/x.png",
      "data:image/png;base64,AAAA",
      "blob:abc",
      "asset://localhost/x.png",
      "file:///tmp/x.png",
      "tauri://localhost/x.png",
    ]) {
      expect(needsVaultResolve(url)).toBe(false);
    }
  });

  it("认不出的 scheme 也不当成路径", () => {
    // 反过来写(只排除已知的安全 scheme)的话,`javascript:` 不在名单里就会被
    // 判成"相对路径"送进解析层 —— 那等于把一段脚本当文件名去读盘。
    expect(needsVaultResolve("javascript:alert(1)")).toBe(false);
    expect(needsVaultResolve("vbscript:msgbox(1)")).toBe(false);
    expect(needsVaultResolve("weird-scheme:whatever")).toBe(false);
  });

  it("协议相对地址是网络地址,不是 vault 里的文件", () => {
    expect(needsVaultResolve("//cdn.example.com/x.png")).toBe(false);
  });

  it("空地址和纯锚点不解析", () => {
    expect(needsVaultResolve("")).toBe(false);
    expect(needsVaultResolve("   ")).toBe(false);
    expect(needsVaultResolve("#section")).toBe(false);
  });

  it("Windows 盘符不当成相对路径", () => {
    // `C:` 命中 scheme 那条规则。它本来就是绝对路径,不需要拼。
    expect(needsVaultResolve("C:\\vault\\attachments\\x.png")).toBe(false);
  });
});

describe("joinNotePath", () => {
  it("把 `..` 折叠掉", () => {
    // 折叠必须在前端做:带 `..` 的路径交给后端,`resolve_in_vaults` 那道闸门会
    // 直接拒 —— 而这条路径其实是合法的,只是没规范化。
    expect(joinNotePath("/vault/a/b", "../../attachments/x.png")).toBe("/vault/attachments/x.png");
    expect(joinNotePath("/vault", "attachments/x.png")).toBe("/vault/attachments/x.png");
    expect(joinNotePath("/vault", "./attachments/./x.png")).toBe("/vault/attachments/x.png");
  });

  it("退到根之后不再往上爬", () => {
    // 再退一层会把根吃掉,拼出来的相对路径反而指到进程 cwd 去。
    expect(joinNotePath("/vault", "../../../etc/passwd")).toBe("/etc/passwd");
  });

  it("绝对路径不拼进笔记目录", () => {
    // 拼的话会得到 `/vault/a/Users/…`。它在不在 vault 里由后端的 allowlist 判。
    expect(joinNotePath("/vault/a", "/other/x.png")).toBe("/other/x.png");
    // Windows 的两种绝对写法也算绝对。只认 `/` 的话从别的工具导入的笔记里那些
    // `C:\...` 会被拼成 `C:\vault\a\C:\pics\x.png`。
    expect(joinNotePath("C:\\vault\\a", "D:\\pics\\x.png")).toBe("D:\\pics\\x.png");
    expect(joinNotePath("C:\\vault\\a", "\\pics\\x.png")).toBe("\\pics\\x.png");
  });

  it("Windows 路径保留盘符和反斜杠", () => {
    expect(joinNotePath("C:\\vault\\a", "..\\attachments\\x.png")).toBe(
      "C:\\vault\\attachments\\x.png",
    );
  });
});

describe("noteDirOf", () => {
  it("取笔记所在目录", () => {
    expect(noteDirOf("/vault/a/b/Note.md")).toBe("/vault/a/b");
    expect(noteDirOf("C:\\vault\\Note.md")).toBe("C:\\vault");
  });
});

describe("linkFromNote", () => {
  it("链接相对笔记目录,不是 vault 根", () => {
    // 相对 vault 根写的话,子目录里的笔记在别的 markdown 工具里就是断链。
    expect(linkFromNote("/vault", "/vault/Note.md", "attachments/x.png")).toBe("attachments/x.png");
    expect(linkFromNote("/vault", "/vault/a/Note.md", "attachments/x.png")).toBe(
      "../attachments/x.png",
    );
    expect(linkFromNote("/vault", "/vault/a/b/Note.md", "attachments/x.png")).toBe(
      "../../attachments/x.png",
    );
  });

  it("链接一律用正斜杠", () => {
    // markdown 是跨平台的:反斜杠在别的工具里读不出来。
    expect(linkFromNote("C:\\vault", "C:\\vault\\a\\Note.md", "attachments\\x.png")).toBe(
      "../attachments/x.png",
    );
  });
});

describe("attachmentMarkdown", () => {
  it("图片插成图片,其余插成链接", () => {
    expect(attachmentMarkdown("x.png", "image", "attachments/x.png")).toBe(
      "![x.png](attachments/x.png)",
    );
    expect(attachmentMarkdown("x.svg", "svg", "attachments/x.svg")).toBe(
      "![x.svg](attachments/x.svg)",
    );
    // `![](x.pdf)` 只会渲染成一个坏掉的图片框。
    expect(attachmentMarkdown("paper.pdf", "pdf", "attachments/paper.pdf")).toBe(
      "[paper.pdf](attachments/paper.pdf)",
    );
  });

  it("alt 里的方括号洗掉", () => {
    // `]` 会提前闭合 alt,页面上会出现一段字面文本加一条指向别处的链接。
    expect(attachmentMarkdown("a]b[c.png", "image", "attachments/x.png")).toBe(
      "![a-b-c.png](attachments/x.png)",
    );
  });
});

describe("vaultRelativePath", () => {
  it("切掉 vault 前缀,留下给人看的那一段", () => {
    expect(vaultRelativePath("/vault", "/vault/notes/a.md")).toBe("notes/a.md");
    expect(vaultRelativePath("/vault", "/vault/a.md")).toBe("a.md");
  });

  it("vault 结尾带分隔符时不留下开头的斜杠", () => {
    // 不去掉的话切出来是 `/notes/a.md`,显示成一条看着像绝对路径的假路径。
    expect(vaultRelativePath("/vault/", "/vault/notes/a.md")).toBe("notes/a.md");
    expect(vaultRelativePath("/vault///", "/vault/notes/a.md")).toBe("notes/a.md");
    expect(vaultRelativePath("C:\\vault\\", "C:\\vault\\notes\\a.md")).toBe("notes\\a.md");
  });

  it("路径里重复的分隔符也剥干净", () => {
    // vault 拼接处多一个斜杠(`${vault}/${name}` 撞上结尾已有分隔符)在真实
    // 路径里很常见,只剥一个的话剩下的那个会漏到显示上。
    expect(vaultRelativePath("/vault", "/vault//notes/a.md")).toBe("notes/a.md");
    expect(vaultRelativePath("/vault", "/vault///a.md")).toBe("a.md");
  });

  it("没有 vault 就没有相对路径", () => {
    expect(vaultRelativePath(null, "/vault/notes/a.md")).toBeNull();
  });

  it("笔记不在这个 vault 下面时返回 null,而不是编一个", () => {
    // 编出来的相对路径会指到另一个目录去,比直接显示完整路径糟得多。
    expect(vaultRelativePath("/vault", "/other/notes/a.md")).toBeNull();
    expect(vaultRelativePath("/vault", "/va/notes/a.md")).toBeNull();
  });

  it("同名前缀的隔壁目录不算在 vault 里", () => {
    // `/vault-old/a.md` 通得过 startsWith,但切出来的 `-old/a.md` 指向别处。
    expect(vaultRelativePath("/vault", "/vault-old/a.md")).toBeNull();
    expect(vaultRelativePath("/vault/", "/vaults/a.md")).toBeNull();
    expect(vaultRelativePath("C:\\vault", "C:\\vault2\\a.md")).toBeNull();
  });

  it("路径正好就是 vault 自己时返回 null,而不是空串", () => {
    // 空串在面板上是一片空白,看着像加载失败。
    expect(vaultRelativePath("/vault", "/vault")).toBeNull();
    expect(vaultRelativePath("/vault", "/vault/")).toBeNull();
  });
});
