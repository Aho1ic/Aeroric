# Agent Note: 本机构建偶尔留下签名坏掉的宿主，系统一启动就杀掉

Status: implemented

## Problem

2026-11-18 有两次 `pnpm native:build` 之后，审阅版的宿主起不来。

- 12:44：构建报 `sign: bash exited with 138`，即 `codesign` 被 SIGBUS 打断；
- 19:07：构建后 `aeroric-cli` 一运行就被系统杀掉（退出码 137）。`codesign --verify` 报：
  - helper 是"invalid signature (code or signature have been modified)"；
  - bundle 是"code has no resources but signature indicates they must be present"。

两次手动重签后都恢复。两次构建时，旧宿主都还在从同一个 `Contents/MacOS/aeroric-cli` 运行，构建用 `copyFileSync` 原地覆盖它。

我在 Mac 上复现过"旧进程在跑时原地覆盖、重签、再启动"，用的是真实的另一个可执行文件，结果是正常的，没能复现。同时排除了两件事：

- 19:07 那次没有别的构建在并发写同一个 DerivedData（Xcode 的构建记录只有我自己那次）；
- 源码所在的"同步空间"是百度网盘建的同步目录，但当时它的客户端没有在运行。

根因没找到。19:07 那次构建很可能也报了失败，但我只取了输出的最后一行，接着又照常重启了 App，于是启动了一个签名坏掉的宿主。

## Decision

- `build.mjs` 换 helper 时先写 `aeroric-cli.new`，再改名覆盖：
  - 正在运行的旧宿主保留自己的文件；
  - 任何时候读到的都不会是写了一半的文件。
- `sign-local.sh` 每签一项就用 `codesign --verify --strict` 检查，失败或 `codesign` 自己崩了就再签一次，两次都不行才让构建失败。
- 重启审阅版的做法：只在 `native:build` 退出码为 0 时才退出旧 App、启动新 App。构建失败就保留旧 App 并报出失败。

## Alternatives considered

**构建前先退出审阅版。** 最强的理由是去掉"文件正被运行"这个变量。没选：构建要几分钟，这段时间审阅版不可用；而且复现表明运行中的旧文件本身不会把新签名弄坏。

**把构建产物移出"同步空间"。** 最强的理由是同步客户端以后开着时，可能去读写 200 多 MB 的二进制。没选：这次排除了它；改目录会动到很多脚本和文档的路径。以后再出现时，先看同步客户端是否在运行。

## Consequences

- **收益**：偶发的签名失败会自己重试，或者让构建明确失败；审阅版不会再被换成签名坏掉的版本。
- **代价**：每次签名多一次校验，几秒钟。
- **未解决**：`codesign` 为什么会被 SIGBUS 打断，原因不明。再出现时，把 `/tmp` 里的构建日志和 `codesign --verify` 的输出一起留下。
- **验证**：在审阅版运行时跑一次完整构建，签名两项都一次通过、`verify-bundle` 通过，只在构建成功后重启，新宿主正常应答。
