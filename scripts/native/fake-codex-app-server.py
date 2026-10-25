#!/usr/bin/env python3
"""A scripted `codex app-server` for looking at the native session page (N4 D8).

Development only: never a dependency of the product. Speaks the app-server's
line-delimited JSON-RPC on stdio and plays one scenario per prompt, so the
session page shows streamed text, a reasoning summary, a command tool card
with streamed output behind an approval, a plan, usage climbing past 80%,
interrupt, steer, /compact and /review.

    fake-codex-app-server.py --version
    fake-codex-app-server.py app-server
"""
import json
import sys
import threading
import time

VERSION = "codex-cli 0.162.1"
LOCK = threading.Lock()
PENDING = {}  # server request id -> (Event, [response])
STATE = {"next_request": 1000, "threads": 0, "turns": 0, "active": None, "used": 31000}
LIMIT = 200000
DELAY = 0.03


def send(message):
    with LOCK:
        sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
        sys.stdout.flush()


def notify(method, params):
    send({"jsonrpc": "2.0", "method": method, "params": params})


def reply(request, result):
    send({"jsonrpc": "2.0", "id": request["id"], "result": result})


def ask(method, params, timeout=600):
    """Server-initiated request; blocks until the client answers."""
    with LOCK:
        STATE["next_request"] += 1
        rid = STATE["next_request"]
    event, box = threading.Event(), []
    PENDING[rid] = (event, box)
    send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    event.wait(timeout)
    PENDING.pop(rid, None)
    return box[0] if box else None


class Turn:
    def __init__(self, thread, turn_id):
        self.thread = thread
        self.id = turn_id
        self.cancelled = threading.Event()
        self.steers = []

    def alive(self):
        return not self.cancelled.is_set()

    def stream(self, method, item, text):
        for piece in chunks(text):
            if not self.alive():
                return False
            notify(method, {"threadId": self.thread, "turnId": self.id, "itemId": item, "delta": piece})
            time.sleep(DELAY)
        return True


def chunks(text, size=3):
    return [text[i:i + size] for i in range(0, len(text), size)]


def usage(thread, grow):
    STATE["used"] = min(LIMIT - 1000, STATE["used"] + grow)
    notify("thread/tokenUsage/updated", {"threadId": thread, "tokenUsage": {
        "last": {"inputTokens": STATE["used"], "outputTokens": 900},
        "total": {"inputTokens": STATE["used"] + 4200, "outputTokens": 2600},
        "modelContextWindow": LIMIT}})


def plan(thread, turn, done):
    steps = ["读懂需求", "列出项目文件", "给出修改建议"]
    notify("turn/plan/updated", {"threadId": thread, "turnId": turn, "plan": [
        {"step": s, "status": "completed" if i < done else ("inProgress" if i == done else "pending")}
        for i, s in enumerate(steps)]})


def end(turn, status, error=None):
    body = {"id": turn.id, "status": status}
    if error:
        body["error"] = {"message": error}
    notify("turn/completed", {"threadId": turn.thread, "turn": body})
    if STATE["active"] is turn:
        STATE["active"] = None


def scenario(turn, prompt):
    thread, tid = turn.thread, turn.id
    notify("turn/started", {"threadId": thread, "turn": {"id": tid}})
    plan(thread, tid, 0)
    if not turn.stream("item/reasoning/summaryTextDelta", f"r-{tid}",
                       "用户想看项目里有什么。我先列出目录，再根据结果给建议；列目录需要执行命令，按权限先请求批准。"):
        return end(turn, "interrupted")
    text = f"收到：「{prompt[:40]}」。我先看一下项目里有哪些文件。\n\n"
    if not turn.stream("item/agentMessage/delta", f"m1-{tid}", text):
        return end(turn, "interrupted")
    notify("item/completed", {"threadId": thread, "item": {"type": "agentMessage", "id": f"m1-{tid}", "text": text}})
    usage(thread, 9000)
    plan(thread, tid, 1)
    command = "ls -la && git status --short"
    item = {"type": "commandExecution", "id": f"c-{tid}", "command": command, "cwd": ".", "status": "inProgress"}
    notify("item/started", {"threadId": thread, "item": item})
    answer = ask("item/commandExecution/requestApproval",
                 {"threadId": thread, "turnId": tid, "itemId": item["id"], "command": command, "reason": "列出项目文件"})
    if not turn.alive():
        return end(turn, "interrupted")
    decision = (answer or {}).get("decision")
    if decision in ("accept", "acceptForSession"):
        output = "total 24\ndrwxr-xr-x  6 me staff  192 .\n-rw-r--r--  1 me staff 1204 README.md\n" \
                 "-rw-r--r--  1 me staff  880 Package.swift\ndrwxr-xr-x  4 me staff  128 Sources\n" \
                 " M Sources/App/main.swift\n?? notes.txt\n"
        for line in output.splitlines(keepends=True):
            if not turn.alive():
                return end(turn, "interrupted")
            notify("item/commandExecution/outputDelta", {"threadId": thread, "itemId": item["id"], "delta": line})
            time.sleep(0.12)
        notify("item/completed", {"threadId": thread, "item": dict(item, status="completed", exitCode=0,
                                                                    aggregatedOutput=output, durationMs=420)})
        summary = "目录里有 README、Package.swift 和 Sources；`Sources/App/main.swift` 有未提交改动，还有一个未跟踪的 `notes.txt`。\n\n" \
                  "建议：\n1. 先看一下 `main.swift` 的改动是否需要提交；\n2. `notes.txt` 如果只是草稿，可以加进 `.gitignore`。"
    else:
        notify("item/completed", {"threadId": thread, "item": dict(item, status="declined")})
        summary = "好的，我不执行这个命令。你可以告诉我想先看哪个文件，我直接读它。"
    time.sleep(0.2)
    for steer in turn.steers:
        summary = f"（已收到补充：{steer}）\n\n" + summary
    plan(thread, tid, 2)
    if not turn.stream("item/agentMessage/delta", f"m2-{tid}", summary):
        return end(turn, "interrupted")
    notify("item/completed", {"threadId": thread, "item": {"type": "agentMessage", "id": f"m2-{tid}", "text": summary}})
    usage(thread, 60000)
    plan(thread, tid, 3)
    end(turn, "completed")


def short_turn(thread, text, compaction=False):
    STATE["turns"] += 1
    turn = Turn(thread, f"t{STATE['turns']}")
    STATE["active"] = turn

    def run():
        notify("turn/started", {"threadId": thread, "turn": {"id": turn.id}})
        if compaction:
            notify("item/started", {"threadId": thread, "item": {"type": "contextCompaction", "id": f"k-{turn.id}"}})
            time.sleep(0.6)
            notify("item/completed", {"threadId": thread, "item": {"type": "contextCompaction", "id": f"k-{turn.id}"}})
            STATE["used"] = 24000
            usage(thread, 0)
        else:
            turn.stream("item/agentMessage/delta", f"m-{turn.id}", text)
            notify("item/completed", {"threadId": thread, "item": {"type": "agentMessage", "id": f"m-{turn.id}", "text": text}})
        end(turn, "completed")

    threading.Thread(target=run, daemon=True).start()


def handle(message):
    if "method" not in message:  # a response to one of our requests
        waiting = PENDING.get(message.get("id"))
        if waiting:
            waiting[1].append(message.get("result"))
            waiting[0].set()
        return
    method, params = message["method"], message.get("params") or {}
    if "id" not in message:
        return  # notifications such as `initialized`
    if method == "initialize":
        reply(message, {"userAgent": "fake-codex/0.162.1"})
    elif method == "model/list":
        reply(message, {"data": [
            {"id": "fake-main", "model": "gpt-5.5", "displayName": "GPT-5.5", "description": "", "hidden": False,
             "isDefault": True, "defaultReasoningEffort": "medium",
             "supportedReasoningEfforts": [{"reasoningEffort": e, "description": ""} for e in ("low", "medium", "high")]},
            {"id": "fake-mini", "model": "gpt-5.5-mini", "displayName": "GPT-5.5 mini", "description": "", "hidden": False,
             "isDefault": False, "defaultReasoningEffort": "low",
             "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": ""}]}]})
    elif method == "thread/start":
        STATE["threads"] += 1
        thread = f"fake-thread-{int(time.time())}-{STATE['threads']}"
        reply(message, {"thread": {"id": thread, "path": f"/tmp/{thread}.jsonl"}})
        notify("thread/started", {"thread": {"id": thread}})
    elif method == "thread/resume":
        reply(message, {"thread": {"id": params.get("threadId")}})
    elif method == "thread/unsubscribe":
        reply(message, {"status": "unsubscribed"})
    elif method == "turn/start":
        STATE["turns"] += 1
        turn = Turn(params["threadId"], f"t{STATE['turns']}")
        STATE["active"] = turn
        prompt = "".join(part.get("text", "") for part in params.get("input", []))
        reply(message, {"turn": {"id": turn.id}})
        threading.Thread(target=scenario, args=(turn, prompt), daemon=True).start()
    elif method == "turn/steer":
        turn = STATE["active"]
        if turn:
            turn.steers.append("".join(part.get("text", "") for part in params.get("input", [])))
        reply(message, {"turnId": turn.id if turn else None})
    elif method == "turn/interrupt":
        turn = STATE["active"]
        reply(message, {})
        if turn:
            turn.cancelled.set()
            for rid, (event, _) in list(PENDING.items()):
                notify("serverRequest/resolved", {"threadId": turn.thread, "requestId": rid})
                event.set()
    elif method == "thread/compact/start":
        reply(message, {})
        short_turn(params["threadId"], "", compaction=True)
    elif method == "review/start":
        reply(message, {})
        short_turn(params["threadId"], "审查未提交的改动：`main.swift` 新增了启动日志，没有发现明显问题；建议给新函数补一个单测。")
    else:
        send({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601, "message": f"fake codex: {method}"}})


def main():
    if "--version" in sys.argv[1:]:
        print(VERSION)
        return
    if "app-server" not in sys.argv[1:]:
        print("usage: fake-codex-app-server.py app-server | --version", file=sys.stderr)
        sys.exit(2)
    for line in sys.stdin:
        line = line.strip()
        if line:
            handle(json.loads(line))


if __name__ == "__main__":
    main()
