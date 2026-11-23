# Agent Note: D4 第七步：扫码和点链接配对

Status: implemented

## Problem

D4 第五步只能把 Mac 的配对链接复制、粘贴到手机上。实际用法里，Mac 显示的是二维码或一个链接，手机隔着设备拿不到剪贴板（除非开了通用剪贴板）。所以手机这边还缺两样：
- 扫 Mac 上的二维码；
- 在手机上点 `aeroric://pair?code=…` 能直接打开 App。

工程方面有两个限制：
- App 的 Info.plist 是自动生成的（`GENERATE_INFOPLIST_FILE`）。相机权限说明能用 `INFOPLIST_KEY_` 写，URL scheme（`CFBundleURLTypes`）不能。
- 工程文件由 XcodeGen 按 `ios/project.yml` 生成。

## Decision

**扫到码、点开链接，都只是把链接填进配对页，要用户自己点“配对”。**
- 别处来的一个链接或一张图就能让手机去配对，等于让手机连上一台陌生的 Mac。陌生 Mac 能看到这台手机发出的审批，也能往手机上推它的任务。
- 已经配对了一台 Mac 时，配对页写明“配对会替换现在的 \<Mac 名\>”。

**连接器：** 加 `offerPairingLink(_:)`，扫码和打开链接都走它。
- 先用 `CompanionPairingOffer.parse` 校验：读不出来就把原因写进 `lastError`，什么都不填；
- 读得出来就存进 `proposedPairingCode`，`proposedHostName` 记下链接里的 Mac 名；
- 配对页接到这个链接时，把它填进输入框并清掉 `proposedPairingCode`，根视图切到“配对与连接”。

**扫码：** `CompanionPairingScanner` 用 VisionKit 的 `DataScannerViewController`，只认二维码。
- 认到的第一个字符串交给 `offerPairingLink`，然后关掉扫码页；
- 设备不支持（`isSupported`）或相机不可用（`isAvailable`，比如没给权限）时，不显示“扫码”按钮，写明原因；
- 只在 iOS 上编译（`#if os(iOS)`），macOS 上跑的单元测试看不到它。

**链接：** App 的 `onOpenURL` 只接 `aeroric` scheme 的链接，交给 `offerPairingLink`。

**Mac 端画二维码：** Mac 的“生成配对二维码”原来只显示链接文字。`PairingView` 的注释说二维码由调用方渲染，但没有地方真的渲染。这次在 `PairingView` 里用 CoreImage 的 `qrCodeGenerator` 把链接画成二维码：
- 纠错级别 M；
- 按整数倍放大，不插值，保证边缘清楚；
- 生成失败时只显示文字。

CoreImage 是系统框架，不加依赖。

**工程：**
- `project.yml` 用 XcodeGen 的 `info:` 写出 `AeroricCompanionApp/Info.plist`，里面只有 `CFBundleURLTypes`（scheme 是 `aeroric`），其余仍由 Xcode 自动生成、合并；
- 相机说明用 `INFOPLIST_KEY_NSCameraUsageDescription`；
- 用 `xcodegen generate` 重新生成工程。

## Alternatives considered

**点开链接就直接配对。** 没选，理由见上：链接来源不可信，配对必须由用户在看过 Mac 名之后点一下。

**用 AVFoundation 自己搭扫码。** 没选：`DataScannerViewController` 自带取景、对焦和高亮，代码少很多。不支持的老设备照样可以粘贴链接。

**URL scheme 写进 `INFOPLIST_KEY_`。** 做不到：自动生成的 Info.plist 不支持 `CFBundleURLTypes`。

## Consequences

- **收益：** Mac 的配对页有了真的二维码。手机可以扫码，也可以点链接打开 App，再点一下“配对”就完成。
- **局限：**
  - 扫码和链接跳转都要真机才能验证，是 NOT RUN；
  - `aeroric` 这个 scheme 可能被别的 App 抢注，iOS 不保证唯一。用户点“配对”前看得到 Mac 名，这一层只能靠这个兜底。
- **验证：**
  - macOS 新增 1 个测试（`PairingQRCodeTests`）：把配对链接画成二维码，再用系统的 `CIDetector` 读回来，和原链接一字不差。
  - iOS 新增 2 个测试：
    - 递来的链接只填进配对页，不拨号、不配对；读不出来就写原因、什么都不填；取走一次就清掉；
    - 从递来的链接配对成功后，提议清掉。
  - MacBook 上 iOS 包 56 个测试都过；iOS App 在模拟器上编过。构建出的 Info.plist 里同时有 `CFBundleURLTypes`（`aeroric`）、`NSCameraUsageDescription` 和自动生成的键。
  - 工程文件 `ios/AeroricCompanion.xcodeproj` 不进仓库（`.gitignore`），由 `project.yml` 生成。改 `project.yml` 之前先用旧版重新生成过一次，和现有工程一样。`AeroricCompanionApp/Info.plist` 由 XcodeGen 按 `project.yml` 写出，跟着提交。
