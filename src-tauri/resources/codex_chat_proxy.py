#!/usr/bin/env python3
"""Small Responses-to-Chat bridge for Codex custom agents.

Codex 0.145 and newer only accept the Responses wire format.  A number of
OpenAI-compatible gateways, including some public aggregators, only expose
Chat Completions reliably.  This helper keeps Codex on Responses locally and
converts the request/stream at the gateway boundary.

The process is intentionally stdlib-only so generated custom-agent scripts do
not need a package manager or a project-local runtime.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Iterable, Optional, Tuple
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


Json = dict[str, Any]

# Configure logging to stderr so it can be captured
LOG_LEVEL = os.environ.get("AERORIC_PROXY_LOG_LEVEL", "WARNING").upper()
logging.basicConfig(
    level=getattr(logging, LOG_LEVEL, logging.WARNING),
    format="%(asctime)s [%(levelname)s] %(message)s",
    stream=sys.stderr,
)
logger = logging.getLogger("codex-chat-proxy")


def text_from_content(value: Any) -> str:
    if isinstance(value, str):
        return value
    if not isinstance(value, list):
        return "" if value is None else str(value)

    chunks: list[str] = []
    for part in value:
        if isinstance(part, str):
            chunks.append(part)
            continue
        if not isinstance(part, dict):
            continue
        part_type = part.get("type")
        if part_type in {"input_text", "output_text", "text", "refusal"}:
            text = part.get("text")
            if isinstance(text, str):
                chunks.append(text)
        elif part_type == "input_image":
            image_url = part.get("image_url") or part.get("url")
            if isinstance(image_url, dict):
                image_url = image_url.get("url")
            if isinstance(image_url, str):
                chunks.append(f"[image: {image_url}]")
    return "".join(chunks)


def chat_content(value: Any) -> Any:
    if not isinstance(value, list):
        return value if isinstance(value, str) else text_from_content(value)

    parts: list[Json] = []
    for part in value:
        if isinstance(part, str):
            parts.append({"type": "text", "text": part})
            continue
        if not isinstance(part, dict):
            continue
        part_type = part.get("type")
        if part_type in {"input_text", "output_text", "text"}:
            text = part.get("text")
            if isinstance(text, str):
                parts.append({"type": "text", "text": text})
        elif part_type == "input_image":
            image_url = part.get("image_url") or part.get("url")
            if isinstance(image_url, dict):
                image_url = image_url.get("url")
            if isinstance(image_url, str):
                parts.append({"type": "image_url", "image_url": {"url": image_url}})
    return parts or ""


def response_role_to_chat(role: Any) -> str:
    if role in {"developer", "system"}:
        return "system"
    if role == "assistant":
        return "assistant"
    if role == "tool":
        return "tool"
    return "user"


def canonical_json(value: Any) -> str:
    if isinstance(value, str):
        return value
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def custom_tool_parameters(tool: Json) -> Json:
    description = tool.get("description") or (
        "Raw string input for the original custom tool. Preserve formatting exactly."
    )
    return {
        "type": "object",
        "properties": {
            "input": {"type": "string", "description": description},
        },
        "required": ["input"],
    }


def response_tools_to_chat(tools: Any) -> list[Json]:
    if not isinstance(tools, list):
        return []

    converted: list[Json] = []
    seen: set[str] = set()
    for tool in tools:
        if isinstance(tool, str):
            tool = {"type": "custom", "name": tool}
        if not isinstance(tool, dict):
            continue
        tool_type = tool.get("type")
        if tool_type == "function":
            name = tool.get("name")
            if not isinstance(name, str):
                continue
            function = {
                "name": name,
                "description": tool.get("description", ""),
                "parameters": tool.get("parameters") or {
                    "type": "object",
                    "properties": {},
                },
            }
            if "strict" in tool:
                function["strict"] = tool["strict"]
            chat_tool = {"type": "function", "function": function}
        elif tool_type == "custom":
            name = tool.get("name")
            if not isinstance(name, str):
                continue
            chat_tool = {
                "type": "function",
                "function": {
                    "name": name,
                    "description": tool.get("description", ""),
                    "parameters": custom_tool_parameters(tool),
                },
            }
        elif tool_type == "tool_search":
            name = "tool_search"
            chat_tool = {
                "type": "function",
                "function": {
                    "name": name,
                    "description": (
                        "Search and load Codex tools, plugins, connectors, and MCP namespaces."
                    ),
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"},
                            "limit": {"type": "integer"},
                        },
                        "required": ["query"],
                    },
                },
            }
        else:
            # web_search and other hosted tools are not portable through a
            # generic OpenAI Chat Completions gateway.
            continue

        name = chat_tool["function"]["name"]
        if name in seen:
            continue
        seen.add(name)
        converted.append(chat_tool)
    return converted


def append_message(messages: list[Json], message: Json) -> None:
    role = message.get("role")
    if role == "system" and messages and messages[0].get("role") == "system":
        previous = messages[0].get("content", "")
        current = message.get("content", "")
        messages[0]["content"] = f"{previous}\n\n{current}".strip()
        return
    messages.append(message)


def responses_input_to_chat(input_value: Any) -> list[Json]:
    if isinstance(input_value, str):
        return [{"role": "user", "content": input_value}]
    if not isinstance(input_value, list):
        return []

    messages: list[Json] = []
    pending_tool_calls: list[Json] = []
    pending_reasoning: list[str] = []

    def flush_tool_calls() -> None:
        nonlocal pending_tool_calls, pending_reasoning
        if not pending_tool_calls:
            return
        message: Json = {
            "role": "assistant",
            "content": None,
            "tool_calls": pending_tool_calls,
        }
        if pending_reasoning:
            message["reasoning_content"] = "".join(pending_reasoning)
        append_message(messages, message)
        pending_tool_calls = []
        pending_reasoning = []

    for item in input_value:
        if not isinstance(item, dict):
            continue
        item_type = item.get("type")

        if item_type == "reasoning":
            summary = item.get("summary")
            if isinstance(summary, list):
                pending_reasoning.extend(
                    part.get("text", "")
                    for part in summary
                    if isinstance(part, dict) and isinstance(part.get("text"), str)
                )
            continue

        if item_type in {"function_call", "custom_tool_call", "tool_search_call"}:
            name = item.get("name")
            call_id = item.get("call_id") or item.get("id") or f"call_{uuid.uuid4().hex[:12]}"
            if not isinstance(name, str):
                continue
            if item_type == "tool_search_call":
                name = "tool_search"
            arguments = item.get("arguments")
            if arguments is None:
                arguments = item.get("input", "")
            pending_tool_calls.append(
                {
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": canonical_json(arguments),
                    },
                }
            )
            continue

        if item_type in {"function_call_output", "custom_tool_call_output", "tool_search_output"}:
            flush_tool_calls()
            call_id = item.get("call_id") or item.get("id") or ""
            append_message(
                messages,
                {
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": canonical_json(item.get("output", "")),
                },
            )
            continue

        if item_type == "message" or "role" in item or "content" in item:
            flush_tool_calls()
            role = response_role_to_chat(item.get("role"))
            message = {"role": role, "content": chat_content(item.get("content", ""))}
            if role == "assistant" and pending_reasoning:
                message["reasoning_content"] = "".join(pending_reasoning)
                pending_reasoning = []
            append_message(messages, message)
            continue

        if item_type in {"input_text", "input_image", "input_file", "input_audio"}:
            flush_tool_calls()
            role = response_role_to_chat(item.get("role"))
            append_message(
                messages,
                {
                    "role": role,
                    "content": chat_content([item]),
                },
            )

    flush_tool_calls()
    return messages


def responses_to_chat(body: Json) -> Json:
    messages: list[Json] = []
    instructions = body.get("instructions")
    instruction_text = text_from_content(instructions)
    if instruction_text:
        append_message(messages, {"role": "system", "content": instruction_text})
    messages.extend(responses_input_to_chat(body.get("input", [])))

    result: Json = {
        "model": body.get("model", ""),
        "messages": messages,
        "stream": bool(body.get("stream", True)),
    }
    if "max_output_tokens" in body:
        result["max_tokens"] = body["max_output_tokens"]
    for key in ("temperature", "top_p", "stop", "user", "response_format"):
        if key in body:
            result[key] = body[key]

    reasoning = body.get("reasoning")
    if isinstance(reasoning, dict) and isinstance(reasoning.get("effort"), str):
        result["reasoning_effort"] = reasoning["effort"]

    tools = response_tools_to_chat(body.get("tools"))
    if tools:
        result["tools"] = tools
        if "tool_choice" in body:
            choice = body["tool_choice"]
            if isinstance(choice, dict) and choice.get("type") == "function":
                function = choice.get("name") or choice.get("function", {}).get("name")
                result["tool_choice"] = {
                    "type": "function",
                    "function": {"name": function},
                }
            else:
                result["tool_choice"] = choice
        result["parallel_tool_calls"] = body.get("parallel_tool_calls", True)
    result["stream_options"] = {"include_usage": True}
    return result


def upstream_url(base_url: str) -> str:
    value = base_url.strip().rstrip("/")
    if value.endswith("/chat/completions"):
        return value
    if value.endswith("/v1"):
        return value + "/chat/completions"
    return value + "/v1/chat/completions"


def upstream_path(base_url: str, request_path: str) -> str:
    value = base_url.strip().rstrip("/")
    path = request_path if request_path.startswith("/") else f"/{request_path}"
    if value.endswith("/v1") and path.startswith("/v1/"):
        return value + path[3:]
    return value + path


def sse_events(response: Any) -> Iterable[Tuple[Optional[str], str]]:
    event: Optional[str] = None
    data: list[str] = []
    for raw_line in response:
        line = raw_line.decode("utf-8", "replace").rstrip("\r\n")
        if not line:
            if data:
                yield event, "\n".join(data)
            event = None
            data = []
            continue
        if line.startswith("event:"):
            event = line[6:].strip()
        elif line.startswith("data:"):
            data.append(line[5:].lstrip())
    if data:
        yield event, "\n".join(data)


def response_base(
    response_id: str,
    model: str,
    status: str,
    output: list[Json],
    usage: Optional[Json] = None,
) -> Json:
    result: Json = {
        "id": response_id,
        "object": "response",
        "status": status,
        "output": output,
        "model": model,
    }
    if usage is not None:
        result["usage"] = usage
    return result


def responses_usage(usage: Any) -> Json:
    if not isinstance(usage, dict):
        return {"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}
    input_tokens = usage.get("prompt_tokens", usage.get("input_tokens", 0)) or 0
    output_tokens = usage.get("completion_tokens", usage.get("output_tokens", 0)) or 0
    return {
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": usage.get("total_tokens", input_tokens + output_tokens),
    }


class ResponseStream:
    def __init__(self, handler: BaseHTTPRequestHandler, model: str):
        self.handler = handler
        self.model = model
        self.response_id = "resp_" + uuid.uuid4().hex
        self.message_id = "msg_" + uuid.uuid4().hex
        self.text = ""
        self.reasoning = ""
        self.tools: dict[int, Json] = {}
        self.output: list[Json] = []
        self.started = False
        self.text_started = False
        self.finished = False
        self.usage: Optional[Json] = None

    def send(self, event: str, payload: Json) -> None:
        raw = f"event: {event}\ndata: {json.dumps(payload, ensure_ascii=False, separators=(',', ':'))}\n\n"
        self.handler.wfile.write(raw.encode("utf-8"))
        self.handler.wfile.flush()

    def start(self) -> None:
        if self.started:
            return
        self.started = True
        self.send(
            "response.created",
            {
                "type": "response.created",
                "response": response_base(self.response_id, self.model, "in_progress", []),
            },
        )

    def start_text(self) -> None:
        if self.text_started:
            return
        self.start()
        self.text_started = True
        self.send(
            "response.output_item.added",
            {
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {
                    "id": self.message_id,
                    "type": "message",
                    "status": "in_progress",
                    "role": "assistant",
                    "content": [],
                },
            },
        )
        self.send(
            "response.content_part.added",
            {
                "type": "response.content_part.added",
                "item_id": self.message_id,
                "output_index": 0,
                "content_index": 0,
                "part": {"type": "output_text", "text": "", "annotations": []},
            },
        )

    def handle_chunk(self, chunk: Json) -> None:
        self.start()
        if isinstance(chunk.get("usage"), dict):
            self.usage = responses_usage(chunk["usage"])
        choices = chunk.get("choices")
        if not isinstance(choices, list) or not choices:
            return
        choice = choices[0] if isinstance(choices[0], dict) else {}
        delta = choice.get("delta") if isinstance(choice.get("delta"), dict) else {}

        reasoning = delta.get("reasoning_content") or delta.get("reasoning")
        if isinstance(reasoning, str) and reasoning:
            if not self.reasoning:
                self.send(
                    "response.reasoning_summary_part.added",
                    {
                        "type": "response.reasoning_summary_part.added",
                        "item_id": self.message_id,
                        "output_index": 0,
                        "summary_index": 0,
                    },
                )
            self.reasoning += reasoning
            self.send(
                "response.reasoning_summary_text.delta",
                {
                    "type": "response.reasoning_summary_text.delta",
                    "item_id": self.message_id,
                    "output_index": 0,
                    "summary_index": 0,
                    "delta": reasoning,
                },
            )

        content = delta.get("content")
        if isinstance(content, str) and content:
            self.start_text()
            self.text += content
            self.send(
                "response.output_text.delta",
                {
                    "type": "response.output_text.delta",
                    "item_id": self.message_id,
                    "output_index": 0,
                    "content_index": 0,
                    "delta": content,
                },
            )

        tool_calls = delta.get("tool_calls")
        if isinstance(tool_calls, list):
            for call in tool_calls:
                if not isinstance(call, dict):
                    continue
                index = call.get("index", 0)
                try:
                    index = int(index)
                except (TypeError, ValueError):
                    index = 0
                function = call.get("function") if isinstance(call.get("function"), dict) else {}
                state = self.tools.get(index)
                if state is None:
                    state = {
                        "id": call.get("id") or "fc_" + uuid.uuid4().hex,
                        "call_id": call.get("id") or "call_" + uuid.uuid4().hex[:12],
                        "name": function.get("name") or "tool",
                        "arguments": "",
                        "output_index": index + (1 if self.text_started else 0),
                    }
                    self.tools[index] = state
                    self.send(
                        "response.output_item.added",
                        {
                            "type": "response.output_item.added",
                            "output_index": state["output_index"],
                            "item": {
                                "id": state["id"],
                                "type": "function_call",
                                "status": "in_progress",
                                "call_id": state["call_id"],
                                "name": state["name"],
                                "arguments": "",
                            },
                        },
                    )
                arguments = function.get("arguments")
                if isinstance(arguments, str) and arguments:
                    state["arguments"] += arguments
                    self.send(
                        "response.function_call_arguments.delta",
                        {
                            "type": "response.function_call_arguments.delta",
                            "item_id": state["id"],
                            "output_index": state["output_index"],
                            "delta": arguments,
                        },
                    )

    def finish(self) -> None:
        if self.finished:
            return
        self.finished = True
        self.start()
        if self.reasoning:
            self.send(
                "response.reasoning_summary_text.done",
                {
                    "type": "response.reasoning_summary_text.done",
                    "item_id": self.message_id,
                    "output_index": 0,
                    "summary_index": 0,
                    "text": self.reasoning,
                },
            )
        if self.text_started:
            self.send(
                "response.output_text.done",
                {
                    "type": "response.output_text.done",
                    "item_id": self.message_id,
                    "output_index": 0,
                    "content_index": 0,
                    "text": self.text,
                },
            )
            self.send(
                "response.content_part.done",
                {
                    "type": "response.content_part.done",
                    "item_id": self.message_id,
                    "output_index": 0,
                    "content_index": 0,
                    "part": {"type": "output_text", "text": self.text, "annotations": []},
                },
            )
            self.output.append(
                {
                    "id": self.message_id,
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": self.text, "annotations": []}],
                }
            )
            self.send(
                "response.output_item.done",
                {
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": self.output[-1],
                },
            )
        for state in self.tools.values():
            item = {
                "id": state["id"],
                "type": "function_call",
                "status": "completed",
                "call_id": state["call_id"],
                "name": state["name"],
                "arguments": state["arguments"],
            }
            self.output.append(item)
            self.send(
                "response.function_call_arguments.done",
                {
                    "type": "response.function_call_arguments.done",
                    "item_id": state["id"],
                    "output_index": state["output_index"],
                    "arguments": state["arguments"],
                },
            )
            self.send(
                "response.output_item.done",
                {
                    "type": "response.output_item.done",
                    "output_index": state["output_index"],
                    "item": item,
                },
            )
        self.send(
            "response.completed",
            {
                "type": "response.completed",
                "response": response_base(
                    self.response_id,
                    self.model,
                    "completed",
                    self.output,
                    self.usage,
                ),
            },
        )

    def failed(self, message: str, error_type: str = "upstream_error") -> None:
        if self.finished:
            return
        self.finished = True
        self.start()
        logger.error("response.failed: type=%s message=%s", error_type, message)
        self.send(
            "response.failed",
            {
                "type": "response.failed",
                "response": response_base(self.response_id, self.model, "failed", self.output),
                "error": {"type": error_type, "message": message},
            },
        )


class ProxyHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args: Any) -> None:
        return

    def do_GET(self) -> None:
        # Keep model discovery usable through the bridge as well.
        upstream = os.environ.get("AERORIC_UPSTREAM_BASE_URL", "")
        target = upstream_path(upstream, self.path)
        logger.info("GET %s -> %s", self.path, target)
        try:
            response = urlopen(Request(target, headers=self.forward_headers()), timeout=30)
            payload = response.read()
            self.send_response(response.status)
            self.send_header("Content-Type", response.headers.get("Content-Type", "application/json"))
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
        except Exception as error:
            logger.error("GET error: %s", error)
            self.send_error(502, str(error))

    def do_POST(self) -> None:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length)
        try:
            body = json.loads(raw.decode("utf-8"))
            if not isinstance(body, dict):
                raise ValueError("request body must be a JSON object")
        except Exception as error:
            self.send_error(400, str(error))
            return

        if not self.path.endswith("/responses"):
            self.send_error(404, "only /v1/responses is supported")
            return

        target = upstream_url(os.environ.get("AERORIC_UPSTREAM_BASE_URL", ""))
        request = Request(
            target,
            data=json.dumps(responses_to_chat(body), ensure_ascii=False).encode("utf-8"),
            method="POST",
            headers=self.forward_headers({"Content-Type": "application/json"}),
        )
        stream = ResponseStream(self, str(body.get("model", "")))
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "close")
        self.end_headers()

        logger.info("Proxying request to %s (model=%s)", target, body.get("model", ""))
        try:
            with urlopen(request, timeout=300) as upstream:
                content_type = upstream.headers.get("Content-Type", "")
                logger.debug("Upstream response: status=%d content-type=%s", upstream.status, content_type)
                if "text/event-stream" in content_type:
                    for _event, data in sse_events(upstream):
                        if data.strip() == "[DONE]":
                            stream.finish()
                            return
                        try:
                            chunk = json.loads(data)
                        except json.JSONDecodeError:
                            logger.warning("Failed to parse SSE data: %s", data[:200])
                            continue
                        if isinstance(chunk, dict):
                            if "error" in chunk:
                                error = chunk["error"]
                                error_msg = error.get("message", str(error)) if isinstance(error, dict) else str(error)
                                logger.error("Upstream SSE error: %s", error_msg)
                                stream.failed(error_msg)
                                return
                            stream.handle_chunk(chunk)
                    stream.finish()
                else:
                    payload = json.loads(upstream.read().decode("utf-8"))
                    stream.handle_chunk(
                        {
                            "id": payload.get("id"),
                            "model": payload.get("model", body.get("model", "")),
                            "choices": [
                                {
                                    "delta": payload.get("choices", [{}])[0].get("message", {}),
                                    "finish_reason": payload.get("choices", [{}])[0].get(
                                        "finish_reason", "stop"
                                    ),
                                }
                            ],
                            "usage": payload.get("usage"),
                        }
                    )
                    stream.finish()
        except HTTPError as error:
            details = error.read(2048).decode("utf-8", "replace")
            logger.error("Upstream HTTP error %d: %s", error.code, details[:500])
            stream.failed(f"upstream HTTP {error.code}: {details}")
        except (URLError, TimeoutError, BrokenPipeError, ConnectionError) as error:
            logger.error("Upstream connection error: %s", error)
            stream.failed(f"upstream connection failed: {error}", "connection_error")
        except Exception as error:
            logger.error("Unexpected error: %s", error, exc_info=True)
            stream.failed(str(error))

    def forward_headers(self, extra: Optional[Json] = None) -> dict[str, str]:
        headers = {
            "Authorization": self.headers.get("Authorization")
            or f"Bearer {os.environ.get('OPENAI_API_KEY') or os.environ.get('ANTHROPIC_API_KEY', '')}",
            "Accept": "text/event-stream",
            "User-Agent": "Aeroric-Codex-Chat-Bridge/1",
        }
        if extra:
            headers.update({str(key): str(value) for key, value in extra.items()})
        return headers


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--port-file", required=True)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", 0), ProxyHandler)
    with open(args.port_file, "w", encoding="ascii") as port_file:
        port_file.write(str(server.server_port))
        port_file.flush()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
