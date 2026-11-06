import AppKit
import Carbon.HIToolbox
import RaptorKit

enum KeyMap {
    /// Keys that aren't text: arrows, editing keys, function keys.
    static func special(_ e: NSEvent) -> Key? {
        switch Int(e.keyCode) {
        case kVK_Return, kVK_ANSI_KeypadEnter: return .enter
        case kVK_Tab: return .tab
        case kVK_Delete: return .backspace
        case kVK_Escape: return .escape
        case kVK_UpArrow: return .up
        case kVK_DownArrow: return .down
        case kVK_LeftArrow: return .left
        case kVK_RightArrow: return .right
        case kVK_Home: return .home
        case kVK_End: return .end
        case kVK_PageUp: return .pageUp
        case kVK_PageDown: return .pageDown
        case kVK_ForwardDelete: return .delete
        case kVK_Help: return .insert
        case kVK_F1: return .f(n: 1)
        case kVK_F2: return .f(n: 2)
        case kVK_F3: return .f(n: 3)
        case kVK_F4: return .f(n: 4)
        case kVK_F5: return .f(n: 5)
        case kVK_F6: return .f(n: 6)
        case kVK_F7: return .f(n: 7)
        case kVK_F8: return .f(n: 8)
        case kVK_F9: return .f(n: 9)
        case kVK_F10: return .f(n: 10)
        case kVK_F11: return .f(n: 11)
        case kVK_F12: return .f(n: 12)
        case kVK_F13: return .f(n: 13)
        case kVK_F14: return .f(n: 14)
        case kVK_F15: return .f(n: 15)
        case kVK_F16: return .f(n: 16)
        case kVK_F17: return .f(n: 17)
        case kVK_F18: return .f(n: 18)
        case kVK_F19: return .f(n: 19)
        case kVK_F20: return .f(n: 20)
        default: return nil
        }
    }

    /// The C0 byte for Ctrl plus a symbol key (Ctrl+@ is NUL, Ctrl+[ is ESC, Ctrl+/ is US).
    /// Letters go through the engine, which knows the program's keyboard mode.
    static func controlCharacter(_ c: Character) -> String? {
        switch c {
        case " ", "@", "2": return "\u{00}"
        case "[", "3": return "\u{1B}"
        case "\\", "4": return "\u{1C}"
        case "]", "5": return "\u{1D}"
        case "^", "6": return "\u{1E}"
        case "_", "-", "/", "7": return "\u{1F}"
        case "8", "?": return "\u{7F}"
        default: return nil
        }
    }
}
