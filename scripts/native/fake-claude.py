#!/usr/bin/env python3
"""A scripted Claude Code in stream-json control mode, for looking at the
native session page with a Claude session (N5 D8).

Development only: never a dependency of the product. Speaks the protocol the
Claude Agent SDK speaks (control_request / control_response / stream-json
messages) and plays one scenario per prompt: streamed thinking and text, a
TodoWrite plan, a Bash tool behind an approval with session-scope
suggestions, a subagent, usage, interrupt and /compact.

    fake-claude.py --version
    fake-claude.py --output-format stream-json ... [--resume <id>]
"""
import json
import sys
import threading
import time
import uuid

VERSION = "2.1.299 (Claude Code)"
LOCK = threading.Lock()
PENDING = {}  # our request id -> (Event, [response])
STATE = {"session": None, "used": 21000, "active": None, "next": 0}
LIMIT = 200000
DELAY = 0.03


def send(message):
    with LOCK:
        sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
        sys.stdout.flush()


def ok(request_id, response):
    send({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})


def ask(request):
    with LOCK:
        STATE["next"] += 1
        rid = f"fake_req_{STATE['next']}"
    event, box = threading.Event(), []
    PENDING[rid] = (event, box)
    send({"type": "control_request", "request_id": rid, "request": request})
    event.wait(600)
    PENDING.pop(rid, None)
    return rid, (box[0] if box else None)


class Turn:
    def __init__(self):
        self.cancelled = threading.Event()

    def alive(self):
        return not self.cancelled.is_set()


def stream(turn, message_id, kind, text):
    for i in range(0, len(text), 3):
        if not turn.alive():
            return False
        delta = {"type": "text_delta", "text": text[i:i + 3]} if kind == "text" else \
            {"type": "thinking_delta", "thinking": text[i:i + 3]}
        send({"type": "stream_event", "session_id": STATE["session"], "parent_tool_use_id": None,
              "event": {"type": "content_block_delta", "index": 0, "delta": delta}})
        time.sleep(DELAY)
    return True


def assistant(message_id, blocks):
    send({"type": "assistant", "session_id": STATE["session"], "parent_tool_use_id": None,
          "message": {"id": message_id, "role": "assistant", "content": blocks}})


def tool_result(tool_id, content, error=False):
    send({"type": "user", "session_id": STATE["session"], "parent_tool_use_id": None,
          "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tool_id, "content": content,
                                                   "is_error": error}]}})


def todos(done):
    steps = ["读懂需求", "列出项目文件", "给出修改建议"]
    return [{"content": s, "activeForm": s, "status": "completed" if i < done else ("in_progress" if i == done else "pending")}
            for i, s in enumerate(steps)]


def result(turn, subtype="success"):
    STATE["used"] = min(LIMIT - 1000, STATE["used"] + 40000)
    send({"type": "result", "subtype": subtype, "is_error": subtype != "success", "session_id": STATE["session"],
          "duration_ms": 1200, "duration_api_ms": 900, "num_turns": 1, "total_cost_usd": 0.01,
          "usage": {"input_tokens": 1800, "cache_read_input_tokens": 12000, "output_tokens": 420}})
    if STATE["active"] is turn:
        STATE["active"] = None


def scenario(turn, prompt):
    if prompt.strip() == "/compact":
        send({"type": "system", "subtype": "compact_boundary", "session_id": STATE["session"]})
        STATE["used"] = 18000
        return result(turn)
    m1 = f"msg_{uuid.uuid4().hex[:8]}"
    send({"type": "stream_event", "session_id": STATE["session"], "event": {"type": "message_start", "message": {"id": m1}}})
    if not stream(turn, m1, "thinking", "用户想了解项目现状。先列计划，再看目录；执行命令需要批准。"):
        return result(turn, "error_during_execution")
    text = f"收到：「{prompt[:40]}」。我先列个计划，然后看看项目文件。"
    if not stream(turn, m1, "text", text):
        return result(turn, "error_during_execution")
    assistant(m1, [{"type": "thinking", "thinking": "用户想了解项目现状。先列计划，再看目录；执行命令需要批准。"},
                   {"type": "text", "text": text}])
    todo_id = f"toolu_{uuid.uuid4().hex[:8]}"
    assistant(m1, [{"type": "tool_use", "id": todo_id, "name": "TodoWrite", "input": {"todos": todos(1)}}])
    tool_result(todo_id, "Todos updated")
    bash_id = f"toolu_{uuid.uuid4().hex[:8]}"
    command = "ls -la && git status --short"
    assistant(m1, [{"type": "tool_use", "id": bash_id, "name": "Bash", "input": {"command": command, "description": "列出项目文件"}}])
    rid, answer = ask({"subtype": "can_use_tool", "tool_name": "Bash", "input": {"command": command}, "tool_use_id": bash_id,
                       "description": "列出项目文件",
                       "permission_suggestions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "ls:*"}],
                                                   "behavior": "allow", "destination": "localSettings"}]})
    if not turn.alive():
        return result(turn, "error_during_execution")
    if (answer or {}).get("behavior") == "allow":
        tool_result(bash_id, "README.md\nPackage.swift\nSources/\n M Sources/App/main.swift\n?? notes.txt")
        summary = "项目里有 README、Package.swift 和 Sources；`main.swift` 有未提交改动，还有一个未跟踪的 `notes.txt`。"
    else:
        tool_result(bash_id, (answer or {}).get("message", "用户拒绝了这一步"), error=True)
        summary = f"好的，不执行这个命令（{(answer or {}).get('message', '已拒绝')}）。你可以告诉我想先看哪个文件。"
    assistant(m1, [{"type": "tool_use", "id": f"{todo_id}b", "name": "TodoWrite", "input": {"todos": todos(2)}}])
    tool_result(f"{todo_id}b", "Todos updated")
    task_id = f"task_{uuid.uuid4().hex[:6]}"
    send({"type": "system", "subtype": "task_started", "task_id": task_id, "description": "检查测试覆盖", "uuid": task_id,
          "session_id": STATE["session"], "task_type": "local_agent"})
    time.sleep(0.8)
    send({"type": "system", "subtype": "task_notification", "task_id": task_id, "status": "completed",
          "summary": "找到 2 个缺测试的函数", "uuid": task_id, "session_id": STATE["session"]})
    m2 = f"msg_{uuid.uuid4().hex[:8]}"
    send({"type": "stream_event", "session_id": STATE["session"], "event": {"type": "message_start", "message": {"id": m2}}})
    final = summary + "\n\n建议：\n1. 先确认 `main.swift` 的改动；\n2. 给子 Agent 找到的两个函数补单测。"
    if not stream(turn, m2, "text", final):
        return result(turn, "error_during_execution")
    assistant(m2, [{"type": "text", "text": final}])
    assistant(m2, [{"type": "tool_use", "id": f"{todo_id}c", "name": "TodoWrite", "input": {"todos": todos(3)}}])
    tool_result(f"{todo_id}c", "Todos updated")
    result(turn)


def handle(message):
    kind = message.get("type")
    if kind == "control_response":
        response = message.get("response") or {}
        waiting = PENDING.get(response.get("request_id"))
        if waiting:
            waiting[1].append(response.get("response"))
            waiting[0].set()
        return
    if kind == "control_request":
        request, rid = message.get("request") or {}, message.get("request_id")
        subtype = request.get("subtype")
        if subtype == "interrupt":
            turn = STATE["active"]
            if turn:
                turn.cancelled.set()
                for pending_id, (event, _) in list(PENDING.items()):
                    send({"type": "control_cancel_request", "request_id": pending_id})
                    event.set()
        if subtype == "get_context_usage":
            return ok(rid, {"totalTokens": STATE["used"], "maxTokens": LIMIT, "categories": []})
        return ok(rid, {})
    if kind == "user":
        if STATE["session"] is None:
            STATE["session"] = STATE.get("resume") or str(uuid.uuid4())
        send({"type": "system", "subtype": "init", "session_id": STATE["session"], "model": STATE.get("model") or "claude-sonnet",
              "permissionMode": "default", "tools": ["Bash", "TodoWrite"]})
        content = message.get("message", {}).get("content")
        prompt = content if isinstance(content, str) else json.dumps(content, ensure_ascii=False)
        turn = Turn()
        STATE["active"] = turn
        threading.Thread(target=scenario, args=(turn, prompt), daemon=True).start()


def main():
    args = sys.argv[1:]
    if "--version" in args:
        print(VERSION)
        return
    if "--resume" in args:
        STATE["resume"] = args[args.index("--resume") + 1]
    if "--model" in args:
        STATE["model"] = args[args.index("--model") + 1]
    for line in sys.stdin:
        line = line.strip()
        if line:
            handle(json.loads(line))


if __name__ == "__main__":
    main()
