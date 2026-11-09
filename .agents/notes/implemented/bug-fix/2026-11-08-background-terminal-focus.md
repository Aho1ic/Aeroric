# Agent Note: 后台状态更新不夺取终端焦点

Status: implemented

## Problem

`TaskWorkspaceView` 在选中任务从运行中变为 `input_required` 时，无条件在下一个主线程队列里调用 `makeFirstResponder`。状态泵可能在 Aeroric 位于后台时收到这个事件；即使它不负责把 App 置前，也会在窗口响应者链上申请键盘输入，违反后台状态更新不夺取输入控制的语义。

此前对旧版（26.1022.1001641）Computer Use 服务的静态证据显示 skyshot 路径会进入 `ComputerUseAppController.activate()`；当前安装版已升级到 26.1027.1001730，不能直接复用旧版反汇编结论。这个修复只收窄 Aeroric 自己的自动聚焦路径，不把外部服务行为误归因于 App。

## Decision

`TaskWorkspaceView` 只在状态变化发生时 App 仍处于活动状态、终端窗口仍是当前键窗口、任务与持有输入租约均匹配时聚焦终端；后台状态变化只展开终端，不调用 `makeFirstResponder`。判定提取为 `TaskTerminalFocusPolicy` 纯策略，并覆盖后台、非键窗口、任务不匹配和无租约场景。

## Alternatives considered

- **删除所有等待状态的自动聚焦**：能彻底避免该 App 路径夺焦点，但会回退用户在前台查看等待任务时的现有体验；保留前台且已是键窗口的聚焦更窄。
- **只检查 `NSApp.isActive`**：理由是改动最小；没有采用，因为窗口可能仍不是用户当前键窗口，而且外部工具短暂激活 App 时该条件会被误满足。
- **修改签名的 SkyComputerUseService 或注入隐藏开关**：理由是服务包含激活/焦点保护逻辑；没有采用，因为服务不在本仓库、没有受支持的禁用参数，二进制补丁无法形成可维护或可验证的 Aeroric 修复。

## Testing

`swift test --package-path macos/AeroricKit --filter TaskPresentationInputGateTests` 通过，4 个测试全部通过。测试覆盖前台键窗口且任务/租约匹配的允许分支，以及 App 后台、窗口非键、任务不匹配和无租约的禁止分支。

## Consequences

后台状态更新不再通过 Aeroric 自己的 `makeFirstResponder` 路径夺取键盘焦点；前台且用户已经在对应键窗口中查看任务时仍保留自动聚焦。代价是 App 后台或窗口已失去键状态时，等待任务只展开终端，用户需要主动回到窗口后再获取输入焦点。

当前 Computer Use 服务仍包含 `SyntheticAppFocusEnforcer`、`SystemFocusStealPreventer` 和 `WindowOrderingObserver` 等焦点/窗口顺序组件；其 JS 包装层的 `get_app_state` 参数仍只有 `app` 与 `disableDiff`，没有公开的 `background` 或 `neverActivate` 选项。当前版本的具体激活分支尚未重新反汇编确认，因此该工具侧风险仍需单独验证，无法由本仓库的策略判定消除。验证报告必须将它与 Aeroric 内部路径分开说明。
