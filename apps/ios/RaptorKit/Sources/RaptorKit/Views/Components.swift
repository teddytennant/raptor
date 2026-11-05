import SwiftUI

/// A dot that breathes while something is in progress or waiting.
public struct StatusDot: View {
    var color: Color
    var pulsing: Bool
    var size: CGFloat = 8
    @State private var on = false

    public init(color: Color, pulsing: Bool, size: CGFloat = 8) {
        self.color = color
        self.pulsing = pulsing
        self.size = size
    }

    public var body: some View {
        ZStack {
            if pulsing {
                Circle().fill(color.opacity(on ? 0 : 0.45)).frame(width: size, height: size).scaleEffect(on ? 2.2 : 1)
            }
            Circle().fill(color).frame(width: size, height: size)
        }
        .frame(width: size * 2, height: size * 2)
        .onAppear {
            guard pulsing else { return }
            withAnimation(.easeOut(duration: 1.1).repeatForever(autoreverses: false)) { on = true }
        }
        .accessibilityHidden(true)
    }
}

/// "● Needs permission" in the state's color.
public struct AgentPill: View {
    var status: AgentStatus
    public init(_ status: AgentStatus) { self.status = status }

    public var body: some View {
        let c = Palette.agent(status.state)
        HStack(spacing: 5) {
            if status.state == .working {
                ProgressView().controlSize(.mini).tint(c)
            } else {
                Circle().fill(c).frame(width: 6, height: 6)
            }
            Text(status.label).font(.caption.weight(.semibold)).foregroundStyle(c).lineLimit(1)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 3)
        .background(c.opacity(0.14), in: Capsule())
        .accessibilityElement(children: .combine)
    }
}

/// A miniature of a window's pane layout, panes tinted by agent state.
public struct LayoutThumbnail: View {
    var window: Window?
    var highlight: String?
    @Environment(\.colorScheme) private var scheme

    public init(_ window: Window?, highlight: String? = nil) {
        self.window = window
        self.highlight = highlight
    }

    public var body: some View {
        Canvas { ctx, size in
            let base = scheme == .dark ? Color.white.opacity(0.08) : Color.black.opacity(0.06)
            let stroke = scheme == .dark ? Color.white.opacity(0.16) : Color.black.opacity(0.12)
            let blocks = window?.blocks ?? []
            if blocks.isEmpty {
                ctx.fill(Path(roundedRect: CGRect(origin: .zero, size: size), cornerRadius: 3), with: .color(base))
                return
            }
            for b in blocks {
                let r = CGRect(x: b.x * size.width + 1, y: b.y * size.height + 1, width: b.w * size.width - 2, height: b.h * size.height - 2)
                let tint = b.agent.map { Palette.agent($0.state) }
                let p = Path(roundedRect: r, cornerRadius: 3)
                ctx.fill(p, with: .color(tint?.opacity(0.28) ?? base))
                let focused = b.id == (highlight ?? window?.focusedBlockId)
                ctx.stroke(p, with: .color(focused ? Palette.accent : stroke), lineWidth: focused ? 1.5 : 0.75)
                var y = r.minY + 4
                for (i, f) in [0.7, 0.45, 0.85, 0.3, 0.6].enumerated() where y + 2 < r.maxY - 3 && i < 5 {
                    ctx.fill(Path(roundedRect: CGRect(x: r.minX + 4, y: y, width: max(0, (r.width - 8) * f), height: 2), cornerRadius: 1),
                             with: .color((tint ?? stroke).opacity(0.6)))
                    y += 4.4
                }
            }
        }
        .accessibilityHidden(true)
    }
}

/// Section header styled like iOS grouped lists, with an optional count.
struct SectionLabel: View {
    var title: String
    var count: Int?
    var body: some View {
        HStack(spacing: 6) {
            Text(title.uppercased()).font(.footnote.weight(.semibold)).foregroundStyle(.secondary)
            if let count { Text("\(count)").font(.footnote).foregroundStyle(.tertiary) }
            Spacer()
        }
        .padding(.top, 10)
        .padding(.leading, 4)
    }
}

extension View {
    /// Card surface used across the home screen.
    func card(tint: Color? = nil) -> some View {
        modifier(CardStyle(tint: tint))
    }
}

private struct CardStyle: ViewModifier {
    var tint: Color?
    @Environment(\.colorScheme) private var scheme
    func body(content: Content) -> some View {
        content
            .padding(14)
            .background {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(tint.map { $0.opacity(scheme == .dark ? 0.10 : 0.08) } ?? (scheme == .dark ? Color.white.opacity(0.05) : Color.white))
            }
            .overlay {
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .strokeBorder(tint.map { $0.opacity(0.35) } ?? (scheme == .dark ? Color.white.opacity(0.08) : Color.black.opacity(0.06)), lineWidth: 1)
            }
            .shadow(color: .black.opacity(scheme == .dark ? 0 : 0.04), radius: 8, y: 2)
    }
}
