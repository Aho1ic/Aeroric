# Agent Note: 输入的引号和破折号按原样发给 Agent

Status: implemented

## Problem

macOS 默认开着"智能引号"和"智能破折号"，所有文本框都会把 `'`、`"` 换成弯引号、把 `--` 换成破折号。Aeroric 的输入框是 SwiftUI `TextEditor`，背后是 `NSTextView`，同样被替换。第二批目检里，提示词中的 `user's` 变成了 `user’s`，只能按 ⌘Z 撤销。发给 Agent 的命令和代码里，弯引号会让命令失效。

## Decision

- 启动时（`AppDelegate.applicationWillFinishLaunching`，任何窗口之前）调用 `PlainTyping.apply`：应用自己的偏好里还没有 `NSAutomaticQuoteSubstitutionEnabled`、`NSAutomaticDashSubstitutionEnabled` 时，写成关。
- 整个应用生效：输入框、审批理由、问答卡、文件编辑器都一样。
- 用户在"编辑 → 替换"里重新打开的，写在同一个键里，以用户的为准，之后不再改。
- 用输入法打出的“”不是替换，不受影响。

## Alternatives considered

- **只关输入框**：影响面最小。没有采用（用户选了推荐）：SwiftUI 的 `TextEditor` 没有关替换的修饰符，只能从视图树里去找 `NSTextView`，脆弱；审批理由、问答卡也是发给 Agent 的。
- **发送前把弯引号改回直引号**：不碰设置。没有采用：会把用户有意打的中文引号也改掉。

## Consequences

- **收益**：输入什么就发什么，命令和代码不会被悄悄改坏。
- **代价**：写在应用自己的偏好里；用户若习惯在 Aeroric 里用智能引号，要在"编辑 → 替换"里手动打开一次。
- **验证**：`PlainTypingTests`（初次关闭、用户的选择保留）；输入框效果靠目检，单元测试碰不到 `TextEditor` 背后的视图。
