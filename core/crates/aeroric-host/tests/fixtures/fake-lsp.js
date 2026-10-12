// P8-05 切片一百一十:宿主 LSP 会话面的 **fake language server**。
//
// 本机没有 `typescript-language-server`(窄表只认 ts/js),所以会话面的机器
// 可测判据落在这个讲 Content-Length 帧的 stdio 应答器上:它按 LSP 协议回答
// initialize/initialized,并在 initialized 之后主动发一条 publishDiagnostics
// 通知(诊断必须经宿主 sink 落 EventLog)。
//
// 用法(由 Rust 测试在 scratch 目录里就地生成):
//     node fake-lsp.js --stdio <diagnostics-uri>
//
// 它只回答形状,不解析语义:每个方法的应答都是固定 JSON,断言钉的是**宿主
// 解析并投影出来的 DTO 形状**,不是 server 的智能。

// 本文件落在 `type: module` 的包范围内 ⇒ 必须用 ESM import(CJS 的
// `require` 在这里是 undefined,而 try/catch 会把它吞掉、记录静默失效)。
import { writeFileSync } from "node:fs";

const args = process.argv.slice(2);
const diagnosticsUri = args.find((arg) => arg.startsWith("file://")) ?? "file:///unused";

let buffer = Buffer.alloc(0);

function send(message) {
  const body = JSON.stringify(message);
  process.stdout.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}

function respond(id, result) {
  send({ jsonrpc: "2.0", id, result });
}

function handle(message) {
  if (message.method === "initialize") {
    respond(message.id, {
      capabilities: { hoverProvider: true, definitionProvider: true },
      serverInfo: { name: "fake-lsp", version: "0" },
    });
    return;
  }
  if (message.method === "initialized") {
    // 主动诊断通知:宿主必须把它翻成 `LspDiagnosticsEvent` 落 EventLog。
    send({
      jsonrpc: "2.0",
      method: "textDocument/publishDiagnostics",
      params: {
        uri: diagnosticsUri,
        diagnostics: [
          {
            range: {
              start: { line: 2, character: 4 },
              end: { line: 2, character: 9 },
            },
            severity: 1,
            source: "fake",
            code: 4242,
            message: "fake diagnostic from the harness server",
          },
        ],
      },
    });
    return;
  }
  if (message.method === "textDocument/didOpen" || message.method === "textDocument/didChange") {
    return;
  }
  // 记录宿主**真的发出去的** params 形状。断言不能从响应体里找——宿主会把
  // server 的应答解析成强类型 DTO,未知字段(如回显的 params)会被丢掉,那种
  // 断言永远是 null 而看不出对错。落盘文件没有这个盲区。
  if (message.method && message.params) {
    try {
      writeFileSync(
        `${diagnosticsUri.replace(/^file:\/\//, "")}.params.json`,
        JSON.stringify({ method: message.method, params: message.params }),
      );
    } catch (error) {
      // 记录失败不影响应答:它是诊断面,不是协议面。
    }
  }
  if (message.method === "shutdown") {
    respond(message.id, null);
    return;
  }
  if (message.method === "exit") {
    process.exit(0);
  }
  if (typeof message.id === "undefined") return;

  switch (message.method) {
    case "textDocument/hover":
      respond(message.id, {
        contents: { kind: "markdown", value: "**fake hover**" },
        range: {
          start: { line: 0, character: 0 },
          end: { line: 0, character: 3 },
        },
      });
      return;
    case "textDocument/definition":
      respond(message.id, [
        {
          uri: "file:///scratch/src/helper.ts",
          range: {
            start: { line: 9, character: 1 },
            end: { line: 9, character: 5 },
          },
        },
      ]);
      return;
    case "textDocument/references":
      respond(message.id, []);
      return;
    case "textDocument/completion":
      respond(message.id, {
        isIncomplete: false,
        items: [
          { label: "fakeCompletion", detail: "fake detail", documentation: "fake docs" },
        ],
      });
      return;
    case "textDocument/signatureHelp":
      respond(message.id, {
        signatures: [
          {
            label: "fake(label: string)",
            documentation: "fake signature",
            parameters: [{ label: "label", documentation: null }],
          },
        ],
        activeSignature: 0,
        activeParameter: 0,
      });
      return;
    case "textDocument/documentSymbol":
      respond(message.id, [
        {
          name: "fakeSymbol",
          kind: 12,
          detail: "function",
          location: {
            uri: "file:///scratch/src/fake.ts",
            range: {
              start: { line: 0, character: 0 },
              end: { line: 0, character: 5 },
            },
          },
        },
      ]);
      return;
    case "textDocument/inlayHint":
      // 回显收到的 params:宿主发的到底是整文档 range(legacy)还是 position
      // (本切片反思前的错形态)必须能被断言看见——两者对 range 语义的请求
      // 是行为分叉,光看 hint 列表看不出。
      respond(message.id, [
        {
          label: "fakeHint",
          position: { line: 0, character: 0 },
          echoedParams: message.params,
        },
      ]);
      return;
    case "workspace/symbol":
      respond(message.id, [
        {
          name: "fakeWorkspaceSymbol",
          kind: 12,
          location: {
            uri: "file:///scratch/src/fake.ts",
            range: {
              start: { line: 3, character: 0 },
              end: { line: 3, character: 4 },
            },
          },
        },
      ]);
      return;
    case "textDocument/rename":
      respond(message.id, {
        changes: {
          "file:///scratch/src/fake.ts": [
            {
              range: {
                start: { line: 0, character: 6 },
                end: { line: 0, character: 13 },
              },
              newText: "renamedByFakeServer",
            },
          ],
        },
      });
      return;
    case "textDocument/codeAction":
      respond(message.id, [
        {
          title: "fake quickfix",
          kind: "quickfix",
          edit: {
            changes: {
              "file:///scratch/src/fake.ts": [
                {
                  range: {
                    start: { line: 0, character: 0 },
                    end: { line: 0, character: 3 },
                  },
                  newText: "fixed",
                },
              ],
            },
          },
        },
      ]);
      return;
    case "workspace/executeCommand":
      respond(message.id, null);
      return;
    default:
      respond(message.id, null);
  }
}

process.stdin.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  for (;;) {
    const separator = buffer.indexOf("\r\n\r\n");
    if (separator < 0) return;
    const header = buffer.slice(0, separator).toString("utf8");
    const match = /content-length:\s*(\d+)/i.exec(header);
    if (!match) return;
    const length = Number(match[1]);
    if (buffer.length < separator + 4 + length) return;
    const body = buffer.slice(separator + 4, separator + 4 + length).toString("utf8");
    buffer = buffer.slice(separator + 4 + length);
    try {
      handle(JSON.parse(body));
    } catch {
      // 解析不了的帧就丢掉:这是应答器,不是被断言的对象。
    }
  }
});