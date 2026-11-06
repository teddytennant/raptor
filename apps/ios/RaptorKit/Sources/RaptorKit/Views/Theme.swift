import CoreText
import SwiftUI

/// A terminal font: PostScript names of its regular, bold, italic and bold
/// italic faces (same ids as the Android app).
public struct TermFont: Identifiable, Sendable {
    public let id: String
    public let name: String
    public let faces: [String]
}

public enum Fonts {
    public static let all: [TermFont] = [
        TermFont(id: "jetbrains-mono", name: "JetBrains Mono", faces: ["JetBrainsMono-Regular", "JetBrainsMono-Bold", "JetBrainsMono-Italic", "JetBrainsMono-BoldItalic"]),
        TermFont(id: "iosevka", name: "Iosevka", faces: ["Iosevka-Term", "Iosevka-Term-Bold", "Iosevka-Term-Italic", "Iosevka-Term-Bold-Italic"]),
        TermFont(id: "fira-code", name: "Fira Code", faces: ["FiraCode-Regular", "FiraCode-Bold", "FiraCode-Regular", "FiraCode-Bold"]),
        TermFont(id: "cascadia-code", name: "Cascadia Code", faces: ["CascadiaCode-Regular", "CascadiaCode-Bold", "CascadiaCode-Italic", "CascadiaCode-BoldItalic"]),
        TermFont(id: "ibm-plex-mono", name: "IBM Plex Mono", faces: ["IBMPlexMono", "IBMPlexMono-Bold", "IBMPlexMono-Italic", "IBMPlexMono-BoldItalic"]),
        TermFont(id: "source-code-pro", name: "Source Code Pro", faces: ["SourceCodePro-Regular", "SourceCodePro-Bold", "SourceCodePro-It", "SourceCodePro-BoldIt"]),
        TermFont(id: "system", name: "System Mono", faces: ["Menlo-Regular", "Menlo-Bold", "Menlo-Italic", "Menlo-BoldItalic"]),
    ]

    public static func byId(_ id: String?) -> TermFont { all.first { $0.id == id } ?? all[0] }

    /// Back-compat: the default font's faces.
    static var names: [String] { all[0].faces }

    static func register() {
        struct Once { static let done: Void = {
            let urls = (Bundle.module.urls(forResourcesWithExtension: "ttf", subdirectory: "Fonts") ?? [])
                + (Bundle.module.urls(forResourcesWithExtension: "ttf", subdirectory: nil) ?? [])
            for url in urls { CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil) }
        }() }
        _ = Once.done
    }
}

public extension Font {
    static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
        Fonts.register()
        return .custom(weight == .bold || weight == .semibold ? "JetBrainsMono-Bold" : "JetBrainsMono-Regular", size: size)
    }
}

/// Raptor's palette. Status colors carry meaning everywhere they appear.
public enum Palette {
    public static let accent = Color(hex: 0x34D399)
    public static let working = Color(hex: 0x38BDF8)
    public static let blocked = Color(hex: 0xF59E0B)
    public static let done = Color(hex: 0x34D399)
    public static let error = Color(hex: 0xF43F5E)
    public static let idle = Color(hex: 0x6B7280)
    public static let connecting = Color(hex: 0xFBBF24)
    public static let terminalBackground = Color(hex: 0x0D1117)
    public static let terminalChrome = Color(hex: 0x161B22)
    public static let terminalKey = Color(hex: 0x21262D)
    public static let terminalText = Color(hex: 0xE6EDF3)
    public static let terminalMuted = Color(hex: 0x8B949E)

    public static func agent(_ s: AgentState?) -> Color {
        switch s {
        case .working: return working
        case .blocked: return blocked
        case .done: return done
        case .error: return error
        case .idle, nil: return idle
        }
    }

    public static func connectivity(_ c: Connectivity) -> Color {
        switch c {
        case .online: return accent
        case .connecting: return connecting
        case .offline: return error.opacity(0.85)
        case .idle: return idle
        }
    }
}

public extension Color {
    init(hex: UInt32, alpha: Double = 1) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255, blue: Double(hex & 0xFF) / 255, opacity: alpha)
    }

    /// From the core's 0xAARRGGBB.
    init(argb: UInt32) {
        self.init(hex: argb & 0xFFFFFF)
    }
}

// MARK: - Themes

/// A terminal color scheme. `ansi == nil` means "use whatever the server sends".
public struct ThemeSpec: Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let background: UInt32
    public let foreground: UInt32
    public let cursor: UInt32
    public let accent: UInt32
    public let ansi: [UInt32]?
}

public enum Themes {
    public static let all: [ThemeSpec] = [
        ThemeSpec(id: "tokyo-night", name: "Tokyo Night", background: 0x1A1B26, foreground: 0xC0CAF5, cursor: 0xC0CAF5, accent: 0x7AA2F7,
                  ansi: [0x15161E, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xA9B1D6, 0x414868, 0xFF899D, 0xAFD67A, 0xE9C582, 0x8DB0FF, 0xC7A9FF, 0x97D8F8, 0xC0CAF5]),
        ThemeSpec(id: "raptor", name: "Raptor", background: 0x0D1117, foreground: 0xE6EDF3, cursor: 0x5EE0A8, accent: 0x5EE0A8,
                  ansi: [0x484F58, 0xFF7B72, 0x3FB950, 0xD29922, 0x58A6FF, 0xBC8CFF, 0x39C5CF, 0xB1BAC4, 0x6E7681, 0xFFA198, 0x56D364, 0xE3B341, 0x79C0FF, 0xD2A8FF, 0x56D4DD, 0xFFFFFF]),
        ThemeSpec(id: "midnight", name: "Midnight", background: 0x0F1626, foreground: 0xD8DEE9, cursor: 0x88C0D0, accent: 0x8AB4F8,
                  ansi: [0x3B4252, 0xE06C75, 0xA3BE8C, 0xE5C07B, 0x81A1C1, 0xB48EAD, 0x88C0D0, 0xE5E9F0, 0x4C566A, 0xEF8891, 0xB5D19E, 0xF0D08C, 0x8AB4F8, 0xC8A2C8, 0x8FBCBB, 0xECEFF4]),
        ThemeSpec(id: "mocha", name: "Catppuccin Mocha", background: 0x1E1E2E, foreground: 0xCDD6F4, cursor: 0xF5E0DC, accent: 0xCBA6F7,
                  ansi: [0x45475A, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xBAC2DE, 0x585B70, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xA6ADC8]),
        ThemeSpec(id: "gruvbox", name: "Gruvbox", background: 0x282828, foreground: 0xEBDBB2, cursor: 0xEBDBB2, accent: 0xFABD2F,
                  ansi: [0x3C3836, 0xCC241D, 0x98971A, 0xD79921, 0x458588, 0xB16286, 0x689D6A, 0xA89984, 0x928374, 0xFB4934, 0xB8BB26, 0xFABD2F, 0x83A598, 0xD3869B, 0x8EC07C, 0xEBDBB2]),
        ThemeSpec(id: "solarized-light", name: "Solarized Light", background: 0xFDF6E3, foreground: 0x586E75, cursor: 0x586E75, accent: 0x268BD2,
                  ansi: [0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5, 0x002B36, 0xCB4B16, 0x586E75, 0x657B83, 0x839496, 0x6C71C4, 0x93A1A1, 0xFDF6E3]),
        ThemeSpec(id: "server", name: "Server Colors", background: 0x0D1117, foreground: 0xE6EDF3, cursor: 0x5EE0A8, accent: 0x5EE0A8, ansi: nil),
    ]

    public static func byId(_ id: String?) -> ThemeSpec { all.first { $0.id == id } ?? all[0] }
}

/// Every chrome color, derived from a terminal's background and foreground so the
/// UI always belongs to the terminal it sits on — including "Server Colors".
public struct Chrome: Equatable, Sendable {
    public var bgHex: UInt32
    public var textHex: UInt32
    public var accentHex: UInt32

    public init(bg: UInt32, text: UInt32, accent: UInt32) {
        bgHex = bg & 0xFFFFFF
        textHex = text & 0xFFFFFF
        accentHex = accent & 0xFFFFFF
    }

    public init(_ t: ThemeSpec) { self.init(bg: t.background, text: t.foreground, accent: t.accent) }

    func mix(_ t: Double) -> Color { Color(hex: blend(bgHex | 0xFF00_0000, textHex | 0xFF00_0000, t) & 0xFFFFFF) }

    public var light: Bool {
        let r = Double((bgHex >> 16) & 0xFF), g = Double((bgHex >> 8) & 0xFF), b = Double(bgHex & 0xFF)
        return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255 > 0.55
    }
    public var bg: Color { Color(hex: bgHex) }
    public var text: Color { Color(hex: textHex) }
    public var accent: Color { Color(hex: accentHex) }
    public var surface: Color { mix(0.05) }
    public var raised: Color { mix(0.09) }
    public var glass: Color { mix(0.12).opacity(0.72) }
    public var glassStroke: Color { text.opacity(light ? 0.10 : 0.09) }
    public var key: Color { mix(0.15) }
    public var hairline: Color { text.opacity(0.08) }
    public var secondary: Color { Color(hex: blend(textHex | 0xFF00_0000, bgHex | 0xFF00_0000, 0.38) & 0xFFFFFF) }
    public var tertiary: Color { Color(hex: blend(textHex | 0xFF00_0000, bgHex | 0xFF00_0000, 0.58) & 0xFFFFFF) }
    public var scheme: ColorScheme { light ? .light : .dark }
    /// A shade darker than the terminals: the title bar and the margin the panes sit in.
    public var outerHex: UInt32 { blend(bgHex | 0xFF00_0000, 0xFF00_0000, light ? 0.06 : 0.3) & 0xFFFFFF }
    public var outer: Color { Color(hex: outerHex) }
}

private struct ChromeKey: EnvironmentKey {
    static let defaultValue = Chrome(Themes.all[0])
}

public extension EnvironmentValues {
    var chrome: Chrome {
        get { self[ChromeKey.self] }
        set { self[ChromeKey.self] = newValue }
    }
}

public extension View {
    /// The translucent surface shared by all floating chrome.
    func glass<S: InsettableShape>(_ shape: S, _ c: Chrome) -> some View {
        self
            .background(c.glass, in: shape)
            .background(.ultraThinMaterial, in: shape)
            .overlay(shape.strokeBorder(c.glassStroke, lineWidth: 1))
            .environment(\.colorScheme, c.scheme)
    }
}

/// A short glyph for what a pane is running, shown on tiles and in the dock.
public func glyph(_ b: Block?) -> String {
    guard let b else { return ">_" }
    let shells: Set<String> = ["bash", "zsh", "fish", "sh", "nu", "dash", "ksh", "login"]
    let p = runningName(b)?.lowercased() ?? ""
    if p.isEmpty || shells.contains(p) { return ">_" }
    if p.hasPrefix("nvim") { return "N" }
    if p == "vim" || p == "vi" { return "V" }
    if p.hasPrefix("claude") || (b.agent?.app.lowercased().contains("claude") ?? false) { return "✳" }
    if p.hasPrefix("codex") { return "◎" }
    if p.hasPrefix("python") { return "Py" }
    if ["node", "bun", "deno"].contains(p) { return "JS" }
    if ["htop", "top", "btop"].contains(p) { return "◔" }
    if ["ssh", "mosh"].contains(p) { return "⇄" }
    if ["git", "lazygit", "tig"].contains(p) { return "±" }
    if !p.contains(where: \.isLetter) { return String(b.name.first(where: \.isLetter) ?? "•").uppercased() }
    return String(p.prefix(1)).uppercased()
}

/// What's running, in words a person would use. Shells and junk names (some tools
/// rename their process to a version number) fall back to the terminal title.
public func runningName(_ b: Block) -> String? {
    let shells: Set<String> = ["bash", "zsh", "fish", "sh", "nu", "dash", "ksh", "tcsh", "login"]
    let p = b.process.trimmingCharacters(in: .whitespaces)
    // An agent run through an interpreter (python, node) goes by the name it reports.
    if let app = b.agent?.app, !app.isEmpty, !shells.contains(p) { return app }
    let junk = p.isEmpty || !p.contains(where: \.isLetter) || shells.contains(p)
    let t = b.name.trimmingCharacters(in: .whitespaces)
    let title: String? = (!t.isEmpty && t != b.shortPwd && !shells.contains(t) && t.contains(where: \.isLetter)) ? t : nil
    if !junk { return p }
    if let title { return title }
    return shells.contains(p) ? p : nil
}

/// "~/src/x · claude"
public func detail(_ b: Block?) -> String {
    guard let b else { return "" }
    return [b.shortPwd.isEmpty ? nil : b.shortPwd, runningName(b)].compactMap { $0 }.uniqued().joined(separator: " · ")
}

/// A rounded tile with a glyph for the running program.
public struct Tile: View {
    var text: String
    var size: CGFloat
    @Environment(\.chrome) private var c
    public init(_ text: String, size: CGFloat = 32) {
        self.text = text
        self.size = size
    }
    public var body: some View {
        RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
            .fill(LinearGradient(colors: [c.accent.opacity(0.22), c.accent.opacity(0.10)], startPoint: .top, endPoint: .bottom))
            .background(c.raised, in: RoundedRectangle(cornerRadius: size * 0.28, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: size * 0.28, style: .continuous).strokeBorder(c.glassStroke, lineWidth: 1))
            .overlay(Text(text).font(.mono(text.count > 1 ? size * 0.36 : size * 0.46, weight: .bold)).foregroundStyle(c.accent))
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}
