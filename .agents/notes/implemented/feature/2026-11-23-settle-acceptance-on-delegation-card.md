# Agent Note: 在委派卡上直接标验收

Status: implemented

## Problem

2026-11-23 第二次复测（`~/Documents/computer-use/2026-11-23-1512-aeroric-review-1123b`）时，委派单元回报后有一条验收（`test -f docs/plan-g.md`）没有自动核对。测试者在主会话的委派卡上点这条验收、右键都没反应，最后打开子会话，在它辅助面板的“进度 → 手动核销”里才标上通过。

验收挂在子会话的检查点上，标验收的按钮只做在那个会话自己的检查点面板里。用户看结果是在主会话的委派卡上，要标一条验收就得先跳到子会话，再跳回来。

## Decision

**委派卡的验收行加“通过”“未通过”两个按钮。**

- **什么时候显示：** 子会话已经回报（卡片不在进行中），并且有子会话；这条验收还没通过，或者通过了但已过期（`DelegationText.canSettle`）。这和子会话“手动核销”列出的是同一批验收。
- **点了做什么：** `LiveSessionStore.settleDelegationAcceptance`：
  1. 用 `checkpoint.setObligation` 标子会话检查点上的那条验收；
  2. 重读这个会话的委派（`delegation.list`）。宿主读的时候会重算卡片；验收全部通过时，还会撤掉“还没有自动核对”的提示（见 `2026-11-23-review-1123-findings`）；
  3. App 用读回来的卡替换屏幕上那张（`LiveSessionModel.updateDelegation`），不用等宿主推送。
- **不改宿主：** 标验收和重算卡片的接口都是现成的，只是 App 这边加了入口。

## Alternatives considered

**卡上点验收就跳到子会话的进度面板。** 没选：还是要来回跳，而用户在主会话看到结果就能判断。

**宿主加一个 `delegation.settleAcceptance`，按委派 id 标。** 没选：标的是子会话检查点上的义务，现成的接口按任务 id 和义务 id 就能定位，验收的 id 就是义务的 id。多一个接口只是换了个名字。

## Consequences

- **收益：** 在主会话的委派卡上就能标验收，提示和“需要我处理”立刻跟着变。
- **局限：**
  - 卡片进行中（子会话还在跑）不给按钮，要等回报；
  - 都还没目检。
- **验证：**
  - Swift 新增 2 个测试：
    - `canSettle` 覆盖六种情况：未核对、未通过、已通过、过期、进行中、没有子会话；
    - store 把子会话的义务标上，并用读回来的卡替换。
  - MacBook 上 `DelegationTextTests`、`LiveSessionStoreTests`、`SessionPolishTests` 通过。App 那层的改动要在 Mac mini 全量验证的构建里编过。
