import AeroricClient
import AppKit
import SwiftUI

/// Infinite canvas, phase 1 (M5): boards of session cards, notes, regions and
/// file cards. Pan with two-finger scroll or space + drag, zoom with pinch or
/// ⌘ + scroll, drag on empty space to marquee-select. Layout is persisted by
/// the host through `CanvasBook`; this view owns only presentation state.
public struct CanvasPage: View {
    let book: CanvasBook
    let resolver: CanvasResolver
    let onOpenSession: (String) -> Void
    let onOpenFile: (String, String) -> Void

    @State private var viewport = CanvasViewport()
    @State private var viewSize: CGSize = .zero
    @State private var selection: Set<String> = []
    @State private var editingNote: String?
    @State private var dragOrigins: [String: CGPoint]?
    @State private var resizeOrigin: (id: String, size: CGSize)?
    @State private var marquee: (start: CGPoint, current: CGPoint)?
    @State private var spaceHeld = false
    @State private var panStart: CanvasViewport?
    @State private var confirmingDelete = false
    @State private var confirmingBoardDelete = false
    @State private var renaming: Renaming?
    @State private var renameText = ""
    @State private var pickingSession = false
    @State private var fileError: String?
    @State private var fittedBoard: String?
    @FocusState private var focused: Bool

    private enum Renaming: Equatable { case board(String), region(String) }

    public init(book: CanvasBook, resolver: CanvasResolver, onOpenSession: @escaping (String) -> Void,
                onOpenFile: @escaping (String, String) -> Void) {
        self.book = book
        self.resolver = resolver
        self.onOpenSession = onOpenSession
        self.onOpenFile = onOpenFile
    }

    public var body: some View {
        ToolSectionScaffold(
            title: "画布", systemImage: "square.on.square.dashed",
            projectName: book.currentBoard.map { $0.name ?? "未命名画布" }, status: .wired,
            toolbar: {
                HStack(spacing: AeroricSpace.s) {
                    boardMenu
                    addMenu
                    HairlineDivider(.vertical).frame(height: 16)
                    zoomControls
                }
            }
        ) {
            canvasArea
        }
        .task { if book.boards.isEmpty { await book.load() } }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await book.reload() }
        }
        .onChange(of: book.boardID) { _, _ in
            selection = []
            editingNote = nil
        }
        .confirmationDialog("删除所选的 \(selection.count) 个节点？", isPresented: $confirmingDelete) {
            Button("删除", role: .destructive) { deleteSelection() }
            Button("取消", role: .cancel) {}
        } message: {
            Text("只删除画布上的卡片，引用的会话和文件不受影响。")
        }
        .confirmationDialog("删除画布“\(book.currentBoard?.name ?? "未命名画布")”？", isPresented: $confirmingBoardDelete) {
            Button("删除画布", role: .destructive) {
                if let id = book.boardID { Task { await book.deleteBoard(id) } }
            }
            Button("取消", role: .cancel) {}
        } message: {
            Text("画布上的 \(book.nodes.count) 个卡片会一起删除；引用的会话和文件不受影响。")
        }
        .alert(renaming.map { if case .board = $0 { "重命名画布" } else { "区域标题" } } ?? "", isPresented: Binding(
            get: { renaming != nil }, set: { if !$0 { renaming = nil } }
        )) {
            TextField("名称", text: $renameText)
            Button("保存") { commitRename() }
            Button("取消", role: .cancel) {}
        }
        .alert("无法添加文件卡", isPresented: Binding(get: { fileError != nil }, set: { if !$0 { fileError = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(fileError ?? "")
        }
        .sheet(isPresented: $pickingSession) {
            SessionPickerSheet(resolver: resolver) { pick in
                pickingSession = false
                if let pick { addSession(pick) }
            }
        }
    }

    // MARK: - Header

    private var boardMenu: some View {
        Menu {
            ForEach(book.boards) { board in
                Button {
                    Task { await book.select(board.id) }
                } label: {
                    if board.id == book.boardID {
                        Label("\(board.name ?? "未命名画布") · \(board.nodeCount)", systemImage: "checkmark")
                    } else {
                        Text("\(board.name ?? "未命名画布") · \(board.nodeCount)")
                    }
                }
            }
            Divider()
            Button("新建画布", systemImage: "plus") { Task { await book.createBoard(name: nil) } }
            Button("重命名当前画布…", systemImage: "pencil") {
                guard let id = book.boardID else { return }
                renameText = book.currentBoard?.name ?? ""
                renaming = .board(id)
            }
            Button("删除当前画布…", systemImage: "trash", role: .destructive) { confirmingBoardDelete = true }
                .disabled(book.boardID == nil)
        } label: {
            Label("画布", systemImage: "rectangle.stack")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityIdentifier("canvas.boards")
    }

    private var addMenu: some View {
        Menu {
            Button("便签", systemImage: "note.text") { add(.note) }
            Button("区域", systemImage: "rectangle.dashed") { add(.region) }
            Button("会话卡…", systemImage: "bubble.left") { pickingSession = true }
            Button("文件卡…", systemImage: "doc") { pickFile() }
            #if DEBUG
            Divider()
            Button("生成 200 个测试便签（调试）") { Task { await generateStressNodes() } }
            #endif
        } label: {
            Label("添加", systemImage: "plus")
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .disabled(book.boardID == nil)
        .accessibilityIdentifier("canvas.add")
    }

    private var zoomControls: some View {
        HStack(spacing: AeroricSpace.xxs) {
            Button { zoom(by: 1 / 1.25) } label: { Image(systemName: "minus") }
                .help("缩小（⌘ + 滚轮 / 捏合）")
            Button { viewport = CanvasViewport(scale: 1, offset: viewport.offset) } label: {
                Text("\(Int((viewport.scale * 100).rounded()))%").monospacedDigit().frame(minWidth: 40)
            }
            .help("恢复 100%")
            .accessibilityIdentifier("canvas.zoom")
            Button { zoom(by: 1.25) } label: { Image(systemName: "plus") }
                .help("放大")
            Button { fitAll() } label: { Image(systemName: "arrow.up.left.and.arrow.down.right") }
                .help("适应全部")
                .accessibilityIdentifier("canvas.fit")
        }
        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
    }

    // MARK: - Canvas

    private var canvasArea: some View {
        GeometryReader { proxy in
            ZStack(alignment: .topLeading) {
                DotGrid(viewport: viewport)
                    .contentShape(Rectangle())
                    .gesture(backgroundDrag)
                    .onTapGesture {
                        selection = []
                        editingNote = nil
                        focused = true
                    }
                worldLayer
                if let marquee {
                    let rect = rect(marquee.start, marquee.current)
                    Rectangle()
                        .fill(AeroricColor.accent.opacity(0.08))
                        .overlay(Rectangle().strokeBorder(AeroricColor.accent.opacity(0.6), lineWidth: 1))
                        .frame(width: rect.width, height: rect.height)
                        .offset(x: rect.minX, y: rect.minY)
                        .allowsHitTesting(false)
                }
                if book.nodes.isEmpty && !book.loading {
                    AeroricEmptyState(systemImage: "square.on.square.dashed", title: "空白画布",
                                      message: "用“添加”放上便签、区域、会话卡或文件卡。双指滑动平移，捏合或 ⌘ + 滚轮缩放。")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                        .allowsHitTesting(false)
                }
            }
            .overlay(alignment: .bottomTrailing) {
                if !book.nodes.isEmpty {
                    MinimapView(nodes: book.nodes, viewport: viewport, viewSize: proxy.size) { world in
                        centre(on: world)
                    }
                    .padding(AeroricSpace.l)
                }
            }
            .background(CanvasEventBridge(
                onPan: { delta in viewport = viewport.panned(by: delta) },
                onZoom: { factor, point in viewport = viewport.zoomed(by: factor, around: point) },
                onSpace: { spaceHeld = $0 }
            ))
            .coordinateSpace(name: "canvas")
            .clipped()
            .onAppear { viewSize = proxy.size }
            .onChange(of: proxy.size) { _, size in viewSize = size }
            .onChange(of: book.nodes.count) { _, _ in fitOnFirstLoad() }
        }
        .background(AeroricColor.canvas)
        .focusable()
        .focusEffectDisabled()
        .focused($focused)
        .onKeyPress(phases: .down) { press in handleKey(press) }
        .accessibilityIdentifier("canvas.area")
    }

    private var worldLayer: some View {
        let visible = CanvasGeometry.visible(book.nodes, in: viewport.visibleWorld(in: viewSize))
        let ordered = visible.filter { $0.kind == .region }.sorted { $0.z < $1.z }
            + visible.filter { $0.kind != .region }.sorted { $0.z < $1.z }
        return ZStack(alignment: .topLeading) {
            ForEach(ordered) { node in
                nodeView(node)
                    .frame(width: node.width, height: node.height)
                    .overlay(alignment: .bottomTrailing) {
                        if selection == [node.id] { resizeHandle(node) }
                    }
                    .offset(x: node.x, y: node.y)
            }
        }
        .frame(width: 1, height: 1, alignment: .topLeading)
        .scaleEffect(viewport.scale, anchor: .topLeading)
        .offset(x: viewport.offset.x, y: viewport.offset.y)
    }

    @ViewBuilder private func nodeView(_ node: CanvasClient.Node) -> some View {
        let selected = selection.contains(node.id)
        Group {
            switch node.kind {
            case .session: SessionNodeView(node: node, resolver: resolver, selected: selected)
            case .note:
                NoteNodeView(node: node, selected: selected, editing: editingNote == node.id) { text in
                    editingNote = nil
                    guard text != (node.content ?? "") else { return }
                    var updated = node
                    updated.content = text
                    Task { await book.update(updated) }
                }
            case .region: RegionNodeView(node: node, selected: selected)
            case .file: FileNodeView(node: node, resolver: resolver, selected: selected)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture(count: 2) { activate(node) }
        .onTapGesture { select(node) }
        .gesture(nodeDrag(node))
        .contextMenu { contextMenu(node) }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("canvas.node.\(node.kind.rawValue).\(node.id)")
    }

    @ViewBuilder private func contextMenu(_ node: CanvasClient.Node) -> some View {
        if node.kind == .note || node.kind == .region {
            Menu("颜色") {
                ForEach(CanvasPalette.names, id: \.self) { name in
                    Button(CanvasPalette.title(name)) {
                        var updated = node
                        updated.color = name
                        Task { await book.update(updated) }
                    }
                }
            }
        }
        if node.kind == .region {
            Button("重命名…") {
                renameText = node.title ?? ""
                renaming = .region(node.id)
            }
        }
        if node.kind == .note { Button("编辑") { editingNote = node.id } }
        if node.kind == .session || node.kind == .file { Button("打开") { activate(node) } }
        Divider()
        Button("删除", role: .destructive) {
            if !selection.contains(node.id) { selection = [node.id] }
            confirmingDelete = true
        }
    }

    private func resizeHandle(_ node: CanvasClient.Node) -> some View {
        Image(systemName: "arrow.down.right")
            .font(.system(size: 9, weight: .bold))
            .foregroundStyle(Color.white)
            .frame(width: 18, height: 18)
            .background(AeroricColor.accent, in: Circle())
            .offset(x: 9, y: 9)
            .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .named("canvas"))
                .onChanged { value in
                    if resizeOrigin?.id != node.id { resizeOrigin = (node.id, CGSize(width: node.width, height: node.height)) }
                    guard let origin = resizeOrigin else { return }
                    let minimum: CGFloat = node.kind == .region ? 160 : 80
                    let width = max(minimum, origin.size.width + value.translation.width / viewport.scale)
                    let height = max(minimum * 0.6, origin.size.height + value.translation.height / viewport.scale)
                    book.preview([CanvasClient.Move(id: node.id, x: node.x, y: node.y, width: width, height: height)])
                }
                .onEnded { _ in
                    resizeOrigin = nil
                    guard let current = book.nodes.first(where: { $0.id == node.id }) else { return }
                    let snap = !NSEvent.modifierFlags.contains(.option)
                    Task {
                        await book.move([CanvasClient.Move(id: current.id, x: current.x, y: current.y,
                                                           width: CanvasGeometry.snap(current.width, enabled: snap),
                                                           height: CanvasGeometry.snap(current.height, enabled: snap))])
                    }
                })
            .help("拖动调整大小")
    }

    // MARK: - Gestures

    private var backgroundDrag: some Gesture {
        DragGesture(minimumDistance: 2, coordinateSpace: .named("canvas"))
            .onChanged { value in
                if spaceHeld {
                    if panStart == nil { panStart = viewport }
                    viewport = panStart!.panned(by: value.translation)
                } else {
                    marquee = (value.startLocation, value.location)
                    let world = viewport.toWorld(rect(value.startLocation, value.location))
                    selection = CanvasGeometry.hits(book.nodes, marquee: world)
                }
            }
            .onEnded { _ in
                panStart = nil
                marquee = nil
                focused = true
            }
    }

    private func nodeDrag(_ node: CanvasClient.Node) -> some Gesture {
        DragGesture(minimumDistance: 3, coordinateSpace: .named("canvas"))
            .onChanged { value in
                guard editingNote == nil else { return }
                if dragOrigins == nil {
                    if !selection.contains(node.id) { selection = [node.id] }
                    dragOrigins = movingSet().reduce(into: [:]) { $0[$1.id] = CGPoint(x: $1.x, y: $1.y) }
                }
                let dx = value.translation.width / viewport.scale
                let dy = value.translation.height / viewport.scale
                book.preview((dragOrigins ?? [:]).map { CanvasClient.Move(id: $0.key, x: $0.value.x + dx, y: $0.value.y + dy) })
            }
            .onEnded { value in
                guard let origins = dragOrigins else { return }
                dragOrigins = nil
                let snap = !NSEvent.modifierFlags.contains(.option)
                let dx = value.translation.width / viewport.scale
                let dy = value.translation.height / viewport.scale
                // Snap the dragged node, shift the rest by the same amount so groups keep their shape.
                let anchor = origins[node.id] ?? .zero
                let snapped = CanvasGeometry.snap(CGPoint(x: anchor.x + dx, y: anchor.y + dy), enabled: snap)
                let shift = CGSize(width: snapped.x - anchor.x, height: snapped.y - anchor.y)
                let moves = origins.map { CanvasClient.Move(id: $0.key, x: $0.value.x + shift.width, y: $0.value.y + shift.height) }
                Task { await book.move(moves) }
            }
    }

    /// The selection plus everything inside selected regions (D2).
    private func movingSet() -> [CanvasClient.Node] {
        let selected = book.nodes.filter { selection.contains($0.id) }
        var ids = Set(selected.map(\.id))
        for region in selected where region.kind == .region {
            ids.formUnion(CanvasGeometry.contained(in: region, among: book.nodes).map(\.id))
        }
        return book.nodes.filter { ids.contains($0.id) }
    }

    private func handleKey(_ press: KeyPress) -> KeyPress.Result {
        guard editingNote == nil else { return .ignored }
        switch press.key {
        case .delete, .deleteForward:
            guard !selection.isEmpty else { return .ignored }
            confirmingDelete = true
            return .handled
        case .upArrow, .downArrow, .leftArrow, .rightArrow:
            guard !selection.isEmpty else { return .ignored }
            let step: CGFloat = press.modifiers.contains(.shift) ? 10 : 1
            let delta: CGSize = switch press.key {
            case .upArrow: CGSize(width: 0, height: -step)
            case .downArrow: CGSize(width: 0, height: step)
            case .leftArrow: CGSize(width: -step, height: 0)
            default: CGSize(width: step, height: 0)
            }
            let moves = movingSet().map { CanvasClient.Move(id: $0.id, x: $0.x + delta.width, y: $0.y + delta.height) }
            Task { await book.move(moves) }
            return .handled
        case .escape:
            selection = []
            return .handled
        default:
            return .ignored
        }
    }

    // MARK: - Actions

    private func select(_ node: CanvasClient.Node) {
        focused = true
        if NSEvent.modifierFlags.contains(.shift) {
            if selection.contains(node.id) { selection.remove(node.id) } else { selection.insert(node.id) }
        } else {
            selection = [node.id]
        }
    }

    private func activate(_ node: CanvasClient.Node) {
        switch node.kind {
        case .note: editingNote = node.id
        case .region:
            renameText = node.title ?? ""
            renaming = .region(node.id)
        case .session:
            if let id = node.taskId, resolver.task(id) != nil { onOpenSession(SessionNavigation.task(id)) }
            else if let id = node.importedId, resolver.imported(id) != nil { onOpenSession(SessionNavigation.imported(id)) }
        case .file:
            if let project = node.projectId, let path = node.path { onOpenFile(project, path) }
        }
    }

    private func add(_ kind: CanvasClient.Kind, configure: (inout CanvasClient.Node) -> Void = { _ in }) {
        let origin = CanvasGeometry.dropOrigin(for: kind, viewport: viewport, viewSize: viewSize, cascade: book.nodes.count)
        let size = CanvasGeometry.defaultSize(kind)
        var node = CanvasClient.Node(boardId: book.boardID ?? "", kind: kind, x: origin.x, y: origin.y,
                                     width: size.width, height: size.height)
        if kind == .note { node.color = "yellow" }
        if kind == .region { node.color = "blue"; node.title = "区域" }
        configure(&node)
        Task {
            if let saved = await book.add(node) {
                selection = [saved.id]
                if kind == .note { editingNote = saved.id }
            }
        }
    }

    private func addSession(_ pick: SessionPickerSheet.Pick) {
        add(.session) { node in
            switch pick {
            case let .task(id): node.taskId = id
            case let .imported(id): node.importedId = id
            }
        }
    }

    private func pickFile() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "放到画布"
        guard panel.runModal() == .OK, let url = panel.url?.standardizedFileURL else { return }
        let projects = resolver.projects()
        guard let project = projects
            .filter({ url.path.hasPrefix(URL(fileURLWithPath: $0.path).standardizedFileURL.path + "/") })
            .max(by: { $0.path.count < $1.path.count }) else {
            fileError = "文件必须位于已打开的项目目录里。"
            return
        }
        let root = URL(fileURLWithPath: project.path).standardizedFileURL.path + "/"
        let relative = String(url.path.dropFirst(root.count))
        add(.file) { node in
            node.projectId = project.id
            node.path = relative
        }
    }

    private func deleteSelection() {
        let ids = selection
        selection = []
        Task { await book.delete(ids) }
    }

    private func commitRename() {
        let text = renameText.trimmingCharacters(in: .whitespacesAndNewlines)
        switch renaming {
        case let .board(id):
            Task { await book.renameBoard(id, name: text.isEmpty ? nil : text) }
        case let .region(id):
            guard var node = book.nodes.first(where: { $0.id == id }) else { break }
            node.title = text.isEmpty ? nil : text
            Task { await book.update(node) }
        case nil:
            break
        }
        renaming = nil
    }

    private func zoom(by factor: CGFloat) {
        viewport = viewport.zoomed(by: factor, around: CGPoint(x: viewSize.width / 2, y: viewSize.height / 2))
    }

    private func fitAll() {
        guard let bounds = CanvasGeometry.bounds(book.nodes) else { return }
        viewport = CanvasViewport.fitting(bounds, in: viewSize)
    }

    private func fitOnFirstLoad() {
        guard fittedBoard != book.boardID, !book.nodes.isEmpty, viewSize != .zero else { return }
        fittedBoard = book.boardID
        fitAll()
    }

    private func centre(on world: CGPoint) {
        viewport = CanvasViewport(scale: viewport.scale, offset: CGPoint(
            x: viewSize.width / 2 - world.x * viewport.scale, y: viewSize.height / 2 - world.y * viewport.scale))
    }

    private func rect(_ a: CGPoint, _ b: CGPoint) -> CGRect {
        CGRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
    }

    #if DEBUG
    private func generateStressNodes() async {
        let origin = viewport.toWorld(CGPoint.zero)
        for index in 0..<200 {
            let node = CanvasClient.Node(boardId: book.boardID ?? "", kind: .note, content: "测试 \(index + 1)",
                                         color: CanvasPalette.names[index % CanvasPalette.names.count],
                                         x: CanvasGeometry.snap(origin.x + CGFloat(index % 20) * 240),
                                         y: CanvasGeometry.snap(origin.y + CGFloat(index / 20) * 180),
                                         width: 220, height: 160)
            _ = await book.add(node)
        }
        fitAll()
    }
    #endif
}

/// Dot grid that thins out when zoomed out, so it never turns into noise.
private struct DotGrid: View {
    let viewport: CanvasViewport

    var body: some View {
        Canvas { context, size in
            var spacing = 24 * viewport.scale
            while spacing < 12 { spacing *= 2 }
            let start = CGPoint(x: viewport.offset.x.truncatingRemainder(dividingBy: spacing),
                                y: viewport.offset.y.truncatingRemainder(dividingBy: spacing))
            let dot = CGSize(width: 1.5, height: 1.5)
            var path = Path()
            var y = start.y
            while y < size.height {
                var x = start.x
                while x < size.width {
                    path.addEllipse(in: CGRect(origin: CGPoint(x: x, y: y), size: dot))
                    x += spacing
                }
                y += spacing
            }
            context.fill(path, with: .color(AeroricColor.separator3))
        }
        .background(AeroricColor.canvas)
    }
}

/// 180×120 overview of every node plus the current viewport; click or drag to jump.
private struct MinimapView: View {
    let nodes: [CanvasClient.Node]
    let viewport: CanvasViewport
    let viewSize: CGSize
    let onJump: (CGPoint) -> Void

    static let size = CGSize(width: 180, height: 120)

    var body: some View {
        let visible = viewport.visibleWorld(in: viewSize)
        let map = CanvasGeometry.minimap(nodes: nodes, visible: visible, size: Self.size)
        Canvas { context, _ in
            for node in nodes {
                let rect = map.toMap(CanvasGeometry.frame(node))
                context.fill(Path(roundedRect: rect, cornerRadius: 1.5),
                             with: .color(node.kind == .region ? AeroricColor.accent.opacity(0.18) : AeroricColor.textTertiary.opacity(0.6)))
            }
            context.stroke(Path(map.toMap(visible)), with: .color(AeroricColor.accent), lineWidth: 1.5)
        }
        .frame(width: Self.size.width, height: Self.size.height)
        .contentShape(Rectangle())
        .gesture(DragGesture(minimumDistance: 0).onChanged { value in onJump(map.toWorld(value.location)) })
        .glassSurface(.floatingCard, in: AeroricRadius.shape(AeroricRadius.m))
        .aeroricElevation(.floating, in: AeroricRadius.shape(AeroricRadius.m))
        .accessibilityLabel("小地图")
        .accessibilityIdentifier("canvas.minimap")
    }
}

/// Pick a task or an imported session to place as a card.
struct SessionPickerSheet: View {
    enum Pick { case task(String), imported(String) }

    let resolver: CanvasResolver
    let onPick: (Pick?) -> Void
    @State private var query = ""

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.m) {
            Text("添加会话卡").font(.system(size: AeroricType.headline, weight: .semibold))
            TextField("搜索会话…", text: $query).textFieldStyle(.roundedBorder)
            List {
                let tasks = resolver.allTasks().filter(matches)
                if !tasks.isEmpty {
                    Section("任务") {
                        ForEach(tasks) { task in
                            Button { onPick(.task(task.id)) } label: { row(task.title, task.agent, task.status) }
                                .buttonStyle(.plain)
                        }
                    }
                }
                let imported = resolver.allImported().filter { query.isEmpty || $0.title.localizedCaseInsensitiveContains(query) }
                if !imported.isEmpty {
                    Section("已导入") {
                        ForEach(imported) { session in
                            Button { onPick(.imported(session.id)) } label: { row(session.title, session.agent, "done") }
                                .buttonStyle(.plain)
                        }
                    }
                }
            }
            .listStyle(.inset)
            HStack {
                Spacer()
                Button("取消") { onPick(nil) }.keyboardShortcut(.cancelAction)
            }
        }
        .padding(AeroricSpace.xl)
        .frame(width: 460, height: 420)
    }

    private func matches(_ task: TaskStore.TaskEntry) -> Bool {
        query.isEmpty || task.title.localizedCaseInsensitiveContains(query)
    }

    private func row(_ title: String, _ agent: String?, _ status: String) -> some View {
        HStack(spacing: AeroricSpace.m) {
            StatusDot(ActivityState(wireStatus: status), label: status)
            AgentGlyph(agent: agent, size: 14)
            Text(title).lineLimit(1)
            Spacer()
        }
        .contentShape(Rectangle())
    }
}
