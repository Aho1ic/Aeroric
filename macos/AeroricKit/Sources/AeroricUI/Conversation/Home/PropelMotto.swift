import AppKit
import SwiftUI

/// 2026-10-28 (新建会话第五轮 · 多巴胺): the wind-line mark beside
/// "Propel ideas forward.", one dopamine colour per word, in Fredoka
/// SemiBold. Light mode uses deeper shades so each word keeps its contrast
/// on white.
struct PropelMotto: View {
    static let words = ["Propel", "ideas", "forward."]
    static let text = words.joined(separator: " ")
    static let colors: [Color] = [
        AeroricColor.dynamic(light: .rgb(0x3D73E6), dark: .rgb(0x5B8DEF)),
        AeroricColor.dynamic(light: .rgb(0x3FA866), dark: .rgb(0x6CC48A)),
        AeroricColor.dynamic(light: .rgb(0xF08A2E), dark: .rgb(0xF5A65B)),
    ]
    static let size: CGFloat = 54

    static var coloured: AttributedString {
        var line = AttributedString()
        for (index, word) in words.enumerated() {
            if index > 0 { line += AttributedString(" ") }
            var run = AttributedString(word)
            run.foregroundColor = colors[index]
            line += run
        }
        return line
    }

    var body: some View {
        HStack(spacing: AeroricSpace.xl) {
            WindMark().frame(width: 46, height: 46)
            Text(Self.coloured)
                .font(Self.font(size: Self.size))
                .kerning(0.5)
                .lineLimit(1)
                .minimumScaleFactor(0.5)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Self.text)
        .accessibilityAddTraits(.isHeader)
        .accessibilityIdentifier("new-session.title")
    }

    /// Fredoka is registered by the app bundle as a variable font; the
    /// weight axis is pinned to 600. Without it (unit tests, a missing
    /// resource) the rounded system face keeps the same character.
    static func font(size: CGFloat) -> Font {
        let weight = 0x7767_6874 // 'wght'
        let descriptor = NSFontDescriptor(fontAttributes: [
            .family: "Fredoka",
            .variation: [NSNumber(value: weight): NSNumber(value: 600)],
        ])
        if let font = NSFont(descriptor: descriptor, size: size), font.familyName == "Fredoka" {
            return Font(font)
        }
        return .system(size: size, weight: .semibold, design: .rounded)
    }
}

/// The Aeroric wind-line mark: two curling gusts and a short trailing line,
/// drawn on a 48-unit grid and coloured like the motto's words.
struct WindMark: View {
    var body: some View {
        GeometryReader { proxy in
            let unit = min(proxy.size.width, proxy.size.height) / 48
            let style = StrokeStyle(lineWidth: 4.8 * unit, lineCap: .round)
            ZStack {
                Self.upper(unit).stroke(PropelMotto.colors[0], style: style)
                Self.middle(unit).stroke(PropelMotto.colors[1], style: style)
                Self.lower(unit).stroke(PropelMotto.colors[2], style: style)
            }
        }
        .accessibilityHidden(true)
    }

    private static func point(_ x: CGFloat, _ y: CGFloat, _ unit: CGFloat) -> CGPoint {
        CGPoint(x: x * unit, y: y * unit)
    }

    /// Runs right, then curls up anticlockwise around (28.5, 10.5).
    static func upper(_ unit: CGFloat) -> Path {
        var path = Path()
        path.move(to: point(6, 17, unit))
        path.addLine(to: point(28.5, 17, unit))
        path.addArc(center: point(28.5, 10.5, unit), radius: 6.5 * unit,
                    startAngle: .degrees(90), endAngle: .degrees(180), clockwise: true)
        return path
    }

    /// Runs right, then curls down clockwise around (37, 32.5).
    static func middle(_ unit: CGFloat) -> Path {
        var path = Path()
        path.move(to: point(6, 26, unit))
        path.addLine(to: point(37, 26, unit))
        path.addArc(center: point(37, 32.5, unit), radius: 6.5 * unit,
                    startAngle: .degrees(-90), endAngle: .degrees(180), clockwise: false)
        return path
    }

    static func lower(_ unit: CGFloat) -> Path {
        var path = Path()
        path.move(to: point(6, 35, unit))
        path.addLine(to: point(20, 35, unit))
        return path
    }
}
