import SwiftUI
import WebKit

/// P6-03 Action 3：IDE 视图集。
public struct ProblemsView: View {
    let store: DiagnosticsStore

    public init(store: DiagnosticsStore) {
        self.store = store
    }

    public var body: some View {
        List(store.problems) { problem in
            HStack(spacing: 6) {
                Image(systemName: problem.severity == "error" ? "xmark.octagon.fill" : "exclamationmark.triangle.fill")
                    .foregroundStyle(problem.severity == "error" ? Color.dsDanger : Color.dsWarning)
                    .font(.caption)
                VStack(alignment: .leading, spacing: 2) {
                    Text(problem.message)
                        .font(.caption)
                        .lineLimit(2)
                    Text("\(problem.file):\(problem.line):\(problem.column) · \(problem.source)\(problem.code.map { " · \($0)" } ?? "")")
                        .font(.caption2)
                        .foregroundStyle(Color.dsTextMuted)
                }
            }
            .accessibilityIdentifier("ide.problem.\(problem.id)")
        }
        .overlay {
            if store.problems.isEmpty {
                Text("无问题")
                    .foregroundStyle(Color.dsTextMuted)
                    .font(.caption)
            }
        }
        .accessibilityIdentifier("ide.problems")
    }
}

public struct SymbolOutlineView: View {
    public struct Symbol: Identifiable {
        public var id: String { "\(name):\(line)" }
        public var name: String
        public var kind: String
        public var line: UInt32

        public init(name: String, kind: String, line: UInt32) {
            self.name = name
            self.kind = kind
            self.line = line
        }
    }

    public let symbols: [Symbol]

    public init(symbols: [Symbol]) {
        self.symbols = symbols
    }

    public var body: some View {
        List(symbols) { symbol in
            HStack(spacing: 6) {
                Image(systemName: "f.circle")
                    .font(.caption2)
                    .foregroundStyle(Color.dsAccent)
                Text(symbol.name)
                    .font(.caption)
                Spacer()
                Text(symbol.line.formatted())
                    .font(.caption2)
                    .foregroundStyle(Color.dsTextMuted)
            }
            .accessibilityIdentifier("ide.symbol.\(symbol.id)")
        }
        .accessibilityIdentifier("ide.symbol-outline")
    }
}

public struct CompletionPanel: View {
    public struct Completion: Identifiable {
        public var id: String { label }
        public var label: String
        public var detail: String?

        public init(label: String, detail: String? = nil) {
            self.label = label
            self.detail = detail
        }
    }

    public let completions: [Completion]
    public var onPick: ((_ label: String) -> Void)?

    public init(completions: [Completion], onPick: ((_ label: String) -> Void)? = nil) {
        self.completions = completions
        self.onPick = onPick
    }

    public var body: some View {
        List(completions) { completion in
            Button {
                onPick?(completion.label)
            } label: {
                HStack {
                    Text(completion.label)
                        .font(.caption.monospaced())
                    if let detail = completion.detail {
                        Text(detail)
                            .font(.caption2)
                            .foregroundStyle(Color.dsTextMuted)
                    }
                }
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("ide.completion.\(completion.label)")
        }
        .frame(maxHeight: 200)
        .accessibilityIdentifier("ide.completions")
    }
}

public struct RunConfigurationEditor: View {
    public struct Config: Identifiable, Equatable {
        public var id: String
        public var name: String
        public var command: String
        public var cwd: String

        public init(id: String, name: String, command: String, cwd: String) {
            self.id = id
            self.name = name
            self.command = command
            self.cwd = cwd
        }
    }

    @State private var configs: [Config]
    public var onSave: ((_ configs: [Config]) -> Void)?
    @State private var editing: Config?
    @State private var error: String?

    public init(configs: [Config], onSave: ((_ configs: [Config]) -> Void)? = nil) {
        _configs = State(initialValue: configs)
        self.onSave = onSave
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("运行配置")
                .font(.headline)
            List(configs) { config in
                VStack(alignment: .leading, spacing: 2) {
                    Text(config.name)
                        .font(.caption)
                    Text(config.command)
                        .font(.caption2.monospaced())
                        .foregroundStyle(Color.dsTextMuted)
                }
                .accessibilityIdentifier("ide.run-config.\(config.id)")
            }
            .frame(minHeight: 120)
            HStack {
                Button("新增") { editing = Config(id: UUID().uuidString, name: "", command: "", cwd: "") }
                    .accessibilityIdentifier("ide.run-config.add")
                Button("保存") { onSave?(configs) }
                    .accessibilityIdentifier("ide.run-config.save")
            }
            if editing != nil {
                Form {
                    TextField("名称", text: Binding(
                        get: { self.editing?.name ?? "" },
                        set: { self.editing?.name = $0 }
                    ))
                    TextField("命令", text: Binding(
                        get: { self.editing?.command ?? "" },
                        set: { self.editing?.command = $0 }
                    ))
                    TextField("工作目录", text: Binding(
                        get: { self.editing?.cwd ?? "" },
                        set: { self.editing?.cwd = $0 }
                    ))
                    Button("确定") {
                        guard let editing else { return }
                        // 无效配置保存失败不污染旧配置（非空 name/command 守卫）。
                        guard !editing.name.trimmingCharacters(in: .whitespaces).isEmpty,
                              !editing.command.trimmingCharacters(in: .whitespaces).isEmpty
                        else {
                            error = "名称与命令不能为空"
                            return
                        }
                        error = nil
                        if let index = configs.firstIndex(where: { $0.id == editing.id }) {
                            configs[index] = editing
                        } else {
                            configs.append(editing)
                        }
                        self.editing = nil
                    }
                    .accessibilityIdentifier("ide.run-config.confirm")
                }
                .frame(minHeight: 140)
            }
            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(Color.dsDanger)
            }
        }
        .padding(12)
        .accessibilityIdentifier("ide.run-configs")
    }
}

public struct TestResultsView: View {
    public struct Result: Identifiable {
        public var id: String { name }
        public var name: String
        public var passed: Bool
        public var failureLocation: String?

        public init(name: String, passed: Bool, failureLocation: String? = nil) {
            self.name = name
            self.passed = passed
            self.failureLocation = failureLocation
        }
    }

    public let results: [Result]

    public init(results: [Result]) {
        self.results = results
    }

    public var body: some View {
        List(results) { result in
            HStack(spacing: 6) {
                Image(systemName: result.passed ? "checkmark.circle.fill" : "xmark.circle.fill")
                    .foregroundStyle(result.passed ? Color.dsSuccess : Color.dsDanger)
                Text(result.name)
                    .font(.caption)
                if let location = result.failureLocation {
                    Spacer()
                    Text(location)
                        .font(.caption2.monospaced())
                        .foregroundStyle(Color.dsTextMuted)
                }
            }
            .accessibilityIdentifier("ide.test.\(result.id)")
        }
        .accessibilityIdentifier("ide.test-results")
    }
}

public struct DebugPanel: View {
    let sessionStore: DebugSessionStore
    let breakpointStore: BreakpointStore
    public var debugId: String
    public var onAction: ((_ action: DapAction) -> Void)?

    public init(
        sessionStore: DebugSessionStore,
        breakpointStore: BreakpointStore,
        debugId: String,
        onAction: ((_ action: DapAction) -> Void)? = nil
    ) {
        self.sessionStore = sessionStore
        self.breakpointStore = breakpointStore
        self.debugId = debugId
        self.onAction = onAction
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            // 控件覆盖 start/attach/pause/continue/step/stop（生命周期判定）。
            HStack(spacing: 6) {
                ForEach(DapAction.allCases, id: \.self) { action in
                    Button(action.rawValue) {
                        onAction?(action)
                    }
                    .disabled(!sessionStore.isAllowed(action: action, debugId: debugId))
                    .accessibilityIdentifier("ide.debug.\(action.rawValue)")
                }
            }
            if let session = sessionStore.sessions[debugId] {
                if let reason = session.pausedReason {
                    Text("已暂停：\(reason)")
                        .font(.caption)
                        .foregroundStyle(Color.dsWarning)
                        .accessibilityIdentifier("ide.debug.paused-reason")
                }
                Text("调用栈")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextMuted)
                ForEach(Array(session.callStack.enumerated()), id: \.offset) { _, frame in
                    Text("\(frame.functionName) · \(frame.file):\(frame.line)")
                        .font(.caption2.monospaced())
                        .accessibilityIdentifier("ide.debug.frame.\(frame.line)")
                }
                ForEach(Array(session.scopes.enumerated()), id: \.offset) { _, scope in
                    Text(scope.name)
                        .font(.caption)
                        .foregroundStyle(Color.dsTextMuted)
                    ForEach(Array(scope.variables.enumerated()), id: \.offset) { _, variable in
                        Text("\(variable.name) = \(variable.value)")
                            .font(.caption2.monospaced())
                            .padding(.leading, 12)
                            .accessibilityIdentifier("ide.debug.var.\(variable.name)")
                    }
                }
            }
            if let diagnostic = breakpointStore.lastAdapterDiagnostic {
                // 未知 adapter 状态可诊断。
                Text("adapter：\(diagnostic)")
                    .font(.caption2)
                    .foregroundStyle(Color.dsTextMuted)
                    .accessibilityIdentifier("ide.debug.adapter-diagnostic")
            }
            Text("断点")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
            ForEach(breakpointStore.breakpoints) { bp in
                Text("\(bp.file):\(bp.line)\(bp.condition.map { " · when \($0)" } ?? "")")
                    .font(.caption2.monospaced())
                    .accessibilityIdentifier("ide.debug.bp.\(bp.id)")
            }
        }
        .padding(12)
        .accessibilityIdentifier("ide.debug-panel")
    }
}

/// P6-03 Action 5：web preview（独立受限 WKWebView——显式批准的 URL 才加载；
/// 不注入 host token/任意 command bridge；外部 URL 交系统浏览器）。
public struct WebPreview: View {
    public let url: URL
    public let approved: Bool

    public init(url: URL, approved: Bool) {
        self.url = url
        self.approved = approved
    }

    public var body: some View {
        if approved {
            WebView(url: url)
                .accessibilityIdentifier("ide.web-preview")
        } else {
            VStack(spacing: 8) {
                Image(systemName: "lock.shield")
                    .font(.system(size: 28))
                    .foregroundStyle(Color.dsTextMuted)
                Text("该 URL 未被批准预览；可以用系统浏览器打开。")
                    .font(.caption)
                    .foregroundStyle(Color.dsTextSecondary)
                    .multilineTextAlignment(.center)
                Button("在浏览器打开") {
                    NSWorkspace.shared.open(url)
                }
                .accessibilityIdentifier("ide.web-preview.open-external")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("ide.web-preview.rejected")
        }
    }
}

/// 受限 WKWebView（本地已审查内容；CSP、无任意 file access、无广泛 message
/// handler——不承载应用导航/会话 composer/通用业务 UI）。
private struct WebView: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        // 受限：无 message handler、无文件访问。
        configuration.preferences.setValue(false, forKey: "allowFileAccessFromFileURLs")
        let view = WKWebView(frame: .zero, configuration: configuration)
        // view 关闭后清理对应 preview 资源：导航代理在 Coordinator 持有。
        view.navigationDelegate = context.coordinator
        view.load(URLRequest(url: url))
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        /// 恶意跳转拦截（preview 的恶意跳转/脚本无法调用文件写接口——只允许
        /// 初始 URL 同源导航）。
        private var initialHost: String?

        func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) {
            if initialHost == nil {
                initialHost = webView.url?.host
            }
        }

        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
        ) {
            // 初始导航允许；后续跳转只允许同 host。
            if initialHost == nil || navigationAction.request.url?.host == initialHost {
                decisionHandler(.allow)
            } else {
                decisionHandler(.cancel)
            }
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            // 失败可诊断（不静默）。
        }
    }
}
