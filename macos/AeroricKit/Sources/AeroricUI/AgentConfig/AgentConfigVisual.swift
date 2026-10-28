import AeroricClient
import SwiftUI

// The Agent configuration page's visual pieces (2026-10-26 polish): rounded
// cards, status chips, expandable rows and a floating save bar (no avatars:
// the user turned them off).
// Shared by the four tabs; colours and radii come from the design tokens only.

// MARK: - Pure presentation (tested)

public extension AgentConfigText {
    /// How a connection reads at a glance, worst first.
    enum ConnectionStatus: Equatable, Sendable {
        case disabled, missingKey, cooling, ready
    }

    static func connectionStatus(enabled: Bool, credential: String?, cooling: Bool) -> ConnectionStatus {
        if !enabled { return .disabled }
        if credential != "stored" { return .missingKey }
        return cooling ? .cooling : .ready
    }

    /// The list row's second line, e.g. `Messages · 3 个模型`.
    static func connectionSubtitle(_ connection: ACC.Connection) -> String {
        let protocols = connection.protocols.map(AgentConnectionsClient.protocolTitle).joined(separator: "/")
        let models = connection.models.isEmpty ? "还没有模型" : "\(connection.models.count) 个模型"
        return protocols.isEmpty ? models : "\(protocols) · \(models)"
    }

    /// Small capsules on a collapsed model row.
    static func modelBadges(_ model: ACC.ModelEntry) -> [String] {
        var badges: [String] = []
        if let window = model.contextWindow { badges.append(contextTitle(window)) }
        if !model.protocols.isEmpty {
            badges.append(model.protocols.map(AgentConnectionsClient.protocolTitle).joined(separator: "/"))
        }
        if !model.reasoningTiers.isEmpty { badges.append("\(model.reasoningTiers.count) 档推理") }
        return badges
    }

    static func contextTitle(_ tokens: UInt64) -> String {
        tokens >= 1_000_000 && tokens % 1_000_000 == 0 ? "\(tokens / 1_000_000)M"
            : tokens >= 1000 ? "\(tokens / 1000)K" : "\(tokens)"
    }

    /// Fuzzy model search: every space-separated word must appear in order as
    /// a subsequence, ignoring case and `-`/`_`/`.`/`/` (so `ds4fl` finds
    /// `deepseek-v4.1-flash`). An empty query matches everything.
    static func fuzzyMatch(_ query: String, _ candidate: String) -> Bool {
        let fold = { (text: String) in
            text.lowercased().filter { !"-_./: ".contains($0) }
        }
        let target = Array(fold(candidate))
        for word in query.lowercased().split(separator: " ") {
            let needle = Array(fold(String(word)))
            var index = 0
            for character in target where index < needle.count && character == needle[index] { index += 1 }
            if index < needle.count { return false }
        }
        return true
    }

    /// The store's probe text starts with 成功 on success.
    static func probeSucceeded(_ probe: String) -> Bool { probe.hasPrefix("成功") }
}

// MARK: - Building blocks

/// The card surface (2026-10-26, user review): the page's own dark canvas
/// rather than the sidebar's grey, set apart by a crisper hairline and a faint
/// top-lit sheen so cards still read as raised.
struct ConfigSurface: ViewModifier {
    let radius: CGFloat

    func body(content: Content) -> some View {
        let shape = AeroricRadius.shape(radius)
        content
            .background {
                shape.fill(AeroricColor.canvas)
                    .overlay(shape.fill(LinearGradient(colors: [AeroricColor.hover, .clear],
                                                       startPoint: .top, endPoint: .bottom)))
            }
            .overlay { shape.strokeBorder(AeroricColor.separator2, lineWidth: 1) }
    }
}

extension View {
    func configSurface(_ radius: CGFloat) -> some View { modifier(ConfigSurface(radius: radius)) }
}

/// A tinted capsule label, e.g. 已存密钥 / 冷却中.
struct StatusChip: View {
    let text: String
    var systemImage: String?
    var tint: Color = AeroricColor.textSecondary

    var body: some View {
        HStack(spacing: AeroricSpace.xs) {
            if let systemImage { Image(systemName: systemImage).font(.system(size: 9, weight: .bold)) }
            Text(text).lineLimit(1)
        }
        .font(.system(size: AeroricType.caption, weight: .medium))
        .foregroundStyle(tint)
        .padding(.horizontal, AeroricSpace.m)
        .padding(.vertical, 3)
        .background(tint.opacity(0.13), in: Capsule(style: .continuous))
    }
}

extension AgentConfigText.ConnectionStatus {
    var tint: Color {
        switch self {
        case .disabled: AeroricColor.textTertiary
        case .missingKey: AeroricColor.danger
        case .cooling: AeroricColor.warning
        case .ready: AeroricColor.success
        }
    }
}

/// A rounded section: optional title and trailing controls above a card.
struct ConfigCard<Trailing: View, Content: View>: View {
    let title: String?
    let trailing: Trailing
    let content: Content

    init(_ title: String? = nil, @ViewBuilder trailing: () -> Trailing = { EmptyView() },
         @ViewBuilder content: () -> Content) {
        self.title = title
        self.trailing = trailing()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            if title != nil || ObjectIdentifier(Trailing.self) != ObjectIdentifier(EmptyView.self) {
                HStack(spacing: AeroricSpace.m) {
                    if let title {
                        Text(title).font(.system(size: AeroricType.headline, weight: .semibold))
                    }
                    Spacer(minLength: 0)
                    trailing
                }
                .padding(.horizontal, AeroricSpace.xs)
            }
            VStack(alignment: .leading, spacing: 0) { content }
                .configSurface(AeroricRadius.l)
        }
    }
}

/// One line inside a `ConfigCard`: label and grey explanation, control on the
/// right; rows after the first get an inset hairline.
struct ConfigRow<Control: View>: View {
    let title: String
    var detail: String?
    var first = false
    let control: Control

    init(_ title: String, detail: String? = nil, first: Bool = false, @ViewBuilder control: () -> Control) {
        self.title = title
        self.detail = detail
        self.first = first
        self.control = control()
    }

    var body: some View {
        HStack(alignment: .center, spacing: AeroricSpace.xl) {
            VStack(alignment: .leading, spacing: 3) {
                Text(title).font(.system(size: AeroricType.body, weight: .medium))
                if let detail {
                    Text(detail)
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: AeroricSpace.l)
            control
        }
        .padding(.horizontal, AeroricSpace.xl)
        .padding(.vertical, AeroricSpace.l)
        .frame(minHeight: 52)
        .overlay(alignment: .top) {
            if !first { HairlineDivider().padding(.leading, AeroricSpace.xl) }
        }
    }
}

/// A rounded text field with an accent ring while focused.
struct ConfigTextField: View {
    let placeholder: String
    @Binding var text: String
    var secure = false
    var monospaced = false
    var width: CGFloat? = 300
    @FocusState private var focused: Bool

    var body: some View {
        Group {
            if secure {
                SecureField(placeholder, text: $text)
            } else {
                TextField(placeholder, text: $text)
            }
        }
        .textFieldStyle(.plain)
        .autocorrectionDisabled()
        .font(monospaced ? AeroricType.mono(AeroricType.body) : .system(size: AeroricType.body))
        .focused($focused)
        .padding(.horizontal, AeroricSpace.m + 2)
        .padding(.vertical, AeroricSpace.s + 1)
        .background(AeroricColor.layer2, in: AeroricRadius.shape(AeroricRadius.s))
        .overlay {
            AeroricRadius.shape(AeroricRadius.s)
                .strokeBorder(focused ? AeroricColor.accent : AeroricColor.separator1, lineWidth: focused ? 1.5 : 1)
        }
        .frame(width: width)
    }
}

/// Multi-select capsules for the three wire protocols (not mutually
/// exclusive, so not a segmented control).
struct ProtocolChips: View {
    @Binding var selection: [String]
    var allowEmpty = false

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            ForEach(AgentConfigText.protocols, id: \.self) { raw in
                Toggle(AgentConnectionsClient.protocolTitle(raw), isOn: Binding(
                    get: { selection.contains(raw) },
                    set: { on in
                        if on { if !selection.contains(raw) { selection.append(raw) } }
                        else if allowEmpty || selection.count > 1 { selection.removeAll { $0 == raw } }
                    }
                ))
                .toggleStyle(CapsuleToggleStyle())
            }
        }
        // Always one line: the chips never wrap or squeeze (2026-10-26).
        .fixedSize()
    }
}

struct CapsuleToggleStyle: ToggleStyle {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func makeBody(configuration: Configuration) -> some View {
        Button {
            withAnimation(AeroricMotion.animation(AeroricMotion.quick, reduceMotion: reduceMotion)) {
                configuration.isOn.toggle()
            }
        } label: {
            HStack(spacing: AeroricSpace.xs) {
                if configuration.isOn { Image(systemName: "checkmark").font(.system(size: 9, weight: .bold)) }
                configuration.label.lineLimit(1)
            }
            .fixedSize()
            .font(.system(size: AeroricType.secondary, weight: .medium))
            .foregroundStyle(configuration.isOn ? AeroricColor.onPrimary : AeroricColor.text)
            .padding(.horizontal, AeroricSpace.l)
            .padding(.vertical, AeroricSpace.xs + 1)
            .background(configuration.isOn ? AeroricColor.primary : AeroricColor.layer2, in: Capsule(style: .continuous))
            .overlay {
                Capsule(style: .continuous)
                    .strokeBorder(configuration.isOn ? Color.clear : AeroricColor.separator1, lineWidth: 1)
            }
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(configuration.isOn ? .isSelected : [])
    }
}

/// Slides up from the bottom while there are unsaved edits.
struct FloatingSaveBar: View {
    let visible: Bool
    let onRevert: () -> Void
    let onSave: () -> Void
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ZStack {
            if visible {
                HStack(spacing: AeroricSpace.l) {
                    Image(systemName: "pencil.circle.fill").foregroundStyle(AeroricColor.warning)
                    Text("有未保存的改动").font(.system(size: AeroricType.secondary, weight: .medium))
                    Button("还原", action: onRevert)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    Button("保存", action: onSave)
                        .buttonStyle(AeroricCapsuleButtonStyle(.primary, compact: true))
                        .keyboardShortcut("s")
                        .accessibilityIdentifier("agent-config.save")
                }
                .padding(.leading, AeroricSpace.xl)
                .padding(.trailing, AeroricSpace.m)
                .padding(.vertical, AeroricSpace.s)
                .glassSurface(.floatingCard, in: Capsule(style: .continuous))
                .shadow(color: AeroricColor.shadow, radius: 16, y: 6)
                .transition(.move(edge: .bottom).combined(with: .opacity))
            }
        }
        .padding(.bottom, AeroricSpace.xl)
        .animation(AeroricMotion.animation(AeroricMotion.standard, reduceMotion: reduceMotion), value: visible)
    }
}

/// A list row's content (the List keeps selection and arrow keys).
struct ConfigListItem: View {
    let title: String
    /// The official logo of the harness behind the row (2026-10-28).
    var harness: String? = nil
    /// Used when there is no harness (a pool, an unknown wire).
    var systemImage: String? = nil
    /// A short muted fact before the tag, e.g. `8 个模型`.
    var detail: String? = nil
    var tag: String? = nil
    var tagTint: Color = AeroricColor.textTertiary

    var body: some View {
        HStack(spacing: AeroricSpace.s) {
            Group {
                if HarnessLogo.asset(harness: harness, agent: nil) != nil {
                    HarnessLogo(harness: harness, agent: nil, size: 14)
                } else {
                    Image(systemName: systemImage ?? "link")
                        .font(.system(size: 12))
                        .foregroundStyle(AeroricColor.textSecondary)
                }
            }
            .frame(width: 16)
            Text(title)
                .font(.system(size: AeroricType.body, weight: .medium))
                .lineLimit(1)
                .truncationMode(.tail)
            Spacer(minLength: AeroricSpace.xs)
            if let detail {
                Text(detail)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
                    .lineLimit(1)
            }
            if let tag {
                Text(tag)
                    .font(.system(size: AeroricType.caption - 1, weight: .medium))
                    .foregroundStyle(tagTint)
                    .padding(.horizontal, AeroricSpace.s)
                    .padding(.vertical, 1)
                    .background(tagTint.opacity(0.13), in: Capsule(style: .continuous))
                    .lineLimit(1)
            }
        }
    }
}

extension AgentConfigText {
    /// The harness a connection's wire speaks to, for its logo.
    static func connectionHarness(_ connection: AgentConnectionsClient.Connection) -> String? {
        if connection.protocols.contains("responses") { return "codex" }
        if connection.protocols.contains("messages") { return "claude" }
        return nil
    }
}

extension AgentConfigText.ConnectionStatus {
    var title: String {
        switch self {
        case .disabled: "已停用"
        case .missingKey: "缺少 key"
        case .cooling: "冷却中"
        case .ready: "正常"
        }
    }
}

/// The left column: a title and a capsule "新建" button above the list.
struct ConfigListColumn<Rows: View>: View {
    let title: String
    let newTitle: String
    let newIdentifier: String
    let onNew: () -> Void
    let rows: Rows

    init(_ title: String, newTitle: String, newIdentifier: String, onNew: @escaping () -> Void,
         @ViewBuilder rows: () -> Rows) {
        self.title = title
        self.newTitle = newTitle
        self.newIdentifier = newIdentifier
        self.onNew = onNew
        self.rows = rows()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack {
                Text(title).font(AeroricType.sectionLabel()).foregroundStyle(AeroricColor.textTertiary)
                Spacer()
                Button(action: onNew) { Label(newTitle, systemImage: "plus") }
                    .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
                    .accessibilityIdentifier(newIdentifier)
            }
            .padding(.horizontal, AeroricSpace.l)
            .padding(.top, AeroricSpace.l)
            rows
        }
    }
}

/// After 拉取列表: pick which fetched models to add (2026-10-26).
struct ModelPickSheet: View {
    @Bindable var store: AgentConfigStore
    @Environment(\.dismiss) private var dismiss

    private var pick: Binding<AgentConfigStore.ModelPick> {
        Binding(get: { store.modelPick ?? .init(available: [], existing: []) },
                set: { store.modelPick = $0 })
    }

    var body: some View {
        let value = pick.wrappedValue
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                Text("选择要添加的模型").font(.system(size: AeroricType.title, weight: .semibold))
                Text("共 \(value.available.count) 个，已添加 \(value.existing.intersection(value.available).count) 个，本次选中 \(value.selected.count) 个")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            HStack(spacing: AeroricSpace.s) {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: "magnifyingglass").foregroundStyle(AeroricColor.textTertiary)
                    TextField("搜索模型（支持模糊匹配，如 ds4fl）", text: pick.query)
                        .textFieldStyle(.plain)
                        .accessibilityIdentifier("agent-config.pick.search")
                    if !value.query.isEmpty {
                        Button { pick.wrappedValue.query = "" } label: { Image(systemName: "xmark.circle.fill") }
                            .buttonStyle(.plain)
                            .foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                .padding(.horizontal, AeroricSpace.l)
                .padding(.vertical, AeroricSpace.s + 1)
                .background(AeroricColor.layer2, in: Capsule(style: .continuous))
                .overlay { Capsule(style: .continuous).strokeBorder(AeroricColor.separator1, lineWidth: 1) }
                Button("全选") { pick.wrappedValue.selectAllVisible() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(value.addable.isEmpty)
                    .accessibilityIdentifier("agent-config.pick.all")
                Button("全不选") { pick.wrappedValue.selectNoneVisible() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(value.selected.isEmpty)
                    .accessibilityIdentifier("agent-config.pick.none")
            }
            ScrollView {
                LazyVStack(spacing: AeroricSpace.xxs) {
                    ForEach(value.visible, id: \.self) { id in
                        row(id, existing: value.existing.contains(id), selected: value.selected.contains(id))
                    }
                    if value.visible.isEmpty {
                        Text(value.available.isEmpty ? "上游没有返回任何模型。" : "没有匹配“\(value.query)”的模型。")
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .padding(.vertical, AeroricSpace.xxxl)
                    }
                }
                .padding(AeroricSpace.xs)
            }
            .scrollIndicators(.hidden)
            .frame(maxHeight: .infinity)
            .configSurface(AeroricRadius.l)
            HStack {
                Spacer()
                Button("取消") { store.modelPick = nil; dismiss() }
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost))
                    .keyboardShortcut(.cancelAction)
                Button(value.selected.isEmpty ? "添加" : "添加 \(value.selected.count) 个") {
                    store.applyModelPick()
                    dismiss()
                }
                .buttonStyle(AeroricCapsuleButtonStyle(.primary))
                .keyboardShortcut(.defaultAction)
                .disabled(value.selected.isEmpty)
                .accessibilityIdentifier("agent-config.pick.add")
            }
        }
        .padding(AeroricSpace.xxl)
        .frame(width: 560, height: 600)
    }

    private func row(_ id: String, existing: Bool, selected: Bool) -> some View {
        HoverCapsuleRow(selected: selected, height: 34, horizontalPadding: AeroricSpace.l) {
            pick.wrappedValue.toggle(id)
        } label: {
            HStack(spacing: AeroricSpace.m) {
                Image(systemName: existing || selected ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(existing ? AeroricColor.textTertiary : selected ? AeroricColor.accent : AeroricColor.textTertiary)
                Text(id).font(AeroricType.mono(AeroricType.secondary)).lineLimit(1)
                Spacer(minLength: 0)
                if existing { StatusChip(text: "已添加", tint: AeroricColor.textTertiary) }
            }
        }
        .disabled(existing)
    }
}
