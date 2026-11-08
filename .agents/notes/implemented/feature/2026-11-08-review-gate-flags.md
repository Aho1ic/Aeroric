# Agent Note: 审查提示里标出削弱门禁的改动

Status: implemented

## Problem

[审查循环](2026-10-30-review-loop.md)给审查者的只有验收条款、约束、自动检查和 diff；diff 超过 60,000 字时只给文件列表。写代码的 Agent 删掉一个失败的测试、给它加 `#[ignore]`、或者在 CI 里加 `continue-on-error`，审查者只能碰巧看到。gstack 的 `review/gate-integrity.md` 把这类改动交给确定性规则标出，再由人逐条处置。

## Decision

- `review/gates.rs::flags` 读本轮完整的 `git diff`（不论是否内联），按固定规则列出：
  - 删掉的测试：整个测试文件被删（路径像测试），或一个文件里删掉的测试比新加的多（`#[test]`、`#[tokio::test`、`func test`、`def test_`、`func Test`、`it(`、`test(` 开头的行）；
  - 跳过测试：新加的行含 `#[ignore`、`XCTSkip`、`.skip(`、`.only(`、`@Disabled`、`pytest.mark.skip` 等；
  - 压掉检查：新加的 `eslint-disable`、`swiftlint:disable`、`@ts-ignore`、`# noqa`、`#[allow(` 等；
  - 放松 CI（`.github/workflows/`、`.gitlab-ci*`、`Makefile`、`package.json` 等）：新加的 `continue-on-error: true`、`|| true`、`--no-verify`、`--skip`，以及删掉又没加回的检查命令（`cargo test`、`clippy`、`pytest`、`lint` 等）。
- 每条带"文件:行"，最多 30 条，多的计数。
- 有标记时，审查提示在"自动检查"后加"门禁完整性"一节，要求审查者逐条判断：有正当理由的在 summary 里说明，没有的作为 blocker 报出来。
- 标记只是问题，不是结论；不改界面。

## Alternatives considered

- **同时在改动面板加胶囊提示**：用户自己也能看到。没有采用（用户选了推荐）：要先出示意图；先让审查者用起来，看误报多不多。
- **让审查者自己找**：不用写规则。没有采用：这正是现状，diff 太长不内联时根本看不到。

## Consequences

- **收益**：删测试、跳测试、放松 CI 一定会被审查者看到并表态，diff 不内联时也一样。
- **代价**：规则是字面匹配，会有误报（比如合理的 `#[allow(clippy::…)]`），靠审查者判断消化；改名的测试按"删一个加一个"抵消，不报。
- **需要重访的信号**：审查里这一节被驳回的比例很高，或用户想在面板里看到。
- **验证**：`review/gates_tests.rs` 三个用例（删除与跳过、改名不报、压检查与放松 CI）；`review::tests::gate_flags_reach_the_reviewer_to_judge`。
