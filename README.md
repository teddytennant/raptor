# Raptor

Open-source clients for [Rex](https://superlogical.com) terminal servers: native
apps for **Mac**, **iPhone/iPad**, **Android**, and **Linux** that connect to the Rex
server you already run, on your Mac or on any box on your tailnet.

> Smaller and quicker than the T. rex.

v0.1.0 is about frontends. They talk to an official Rex server (the Linux build
makes a good headless backend). Raptor's own multiplexer server is planned for v2.

## What's in it

- **Pixel-exact terminals.** Rex streams a Ghostty snapshot of each terminal and
  then raw output. Raptor restores it with
  [libghostty-vt](https://github.com/ghostty-org/ghostty), the same VT engine as
  Ghostty, so colors, wide characters, styles, and scrollback match byte for byte.
  Box-drawing characters are drawn as geometry, so lines join with no gaps.
- **"Needs you" inbox.** Programs that report status (OSC 7501, e.g. coding
  agents) show up at the top: blocked, working, done. You see the last lines of
  output and can reply without opening the terminal. Notifications fire when an
  agent needs you or finishes, and you can reply straight from the
  notification.
- **Fit or mirror.** If nobody else holds a terminal's size, your device takes it
  and the terminal reflows to your screen. If your desktop holds it, Raptor mirrors
  at the desktop's size with pinch-zoom and pan. You can switch in one tap.
- **Built for phones.** A key bar (esc, tab, sticky/lockable ctrl and alt, arrows,
  symbols) and momentum scrolling through history. Gestures go to full-screen
  programs (vim, less, htop) as wheel events. Long-press or double-tap to select,
  then use the system copy/paste menu. Pinch to resize text, with haptics
  throughout. Hardware keyboards work too.
- **Survives bad networks.** The control and data connections resume after a drop
  without losing output (byte-exact RESUME). Connections close while the app is
  in the background and reconnect instantly when it returns. Typed characters are
  batched, so a command plus Enter costs one round trip.
- **Live everything.** Sessions, windows, and pane layouts (drawn as thumbnails)
  update live, along with cwd, foreground process, and which other devices are
  attached.

## Layout

| Path | What | Language |
| --- | --- | --- |
| `crates/raptor-proto` | Rex wire protocol: control API revisions 8 to 10, data handshake, in-band messages, snapshot framing | Rust |
| `crates/raptor-client` | Async client: control WebSocket, resumable data streams, Unix/TCP/TLS endpoints | Rust |
| `crates/raptor-term` | Terminal state on [libghostty-rs](https://github.com/Uzaaft/libghostty-rs): snapshot restore, VT, frames as style runs, selection, scrollback | Rust |
| `crates/raptor-core` | The app engine every frontend shares: servers, reconnects, live state, agent status, terminals | Rust |
| `crates/raptor-ffi` | UniFFI bindings for Swift and Kotlin | Rust |
| `crates/raptor-cli` | Debug tool (`ls`, `dump`, `screen`) | Rust |
| `apps/android` | Android app: Jetpack Compose UI, Canvas terminal view | Kotlin |
| `apps/ios` | iOS/iPadOS app: SwiftUI, Core Text terminal, UIKit input (`RaptorKit` package + app target) | Swift |
| `apps/linux` | Linux app: GTK 4 + libadwaita, Cairo/Pango terminal, tiled panes | Rust |
| `apps/macos` | macOS app: SwiftUI sidebar and settings, AppKit terminal view on the shared `RaptorKit` renderer | Swift |

The Rex protocol is documented by every Rex server at `GET /llms.txt`.

## Building

You need Rust (via rustup for cross-compiling), Zig 0.17 (to build
libghostty-vt), and the SDK for each platform. libghostty-vt is built from the
`vendor/ghostty` submodule, pinned to the commit libghostty-rs's bindings were
generated for; `.cargo/config.toml` points `GHOSTTY_SOURCE_DIR` at it, so builds
never fetch Ghostty over the network. Set `ZIG` if `zig` on your PATH is older.

```sh
git clone --recursive https://github.com/teddytennant/raptor
cargo test --workspace
RAPTOR_TEST_SERVER=my-box cargo test -p raptor-core --test live   # end-to-end against a real Rex server
```

**Android** (SDK, NDK, JDK 21, `cargo install cargo-ndk`):

```sh
scripts/build-android.sh              # Rust core for arm64-v8a + x86_64, plus Kotlin bindings
cd apps/android && ./gradlew :app:assembleDebug
```

**iOS** (Xcode, `brew install xcodegen`):

```sh
scripts/build-apple.sh                # RaptorFFI.xcframework + Swift bindings
cd apps/ios && xcodegen generate && open Raptor.xcodeproj
```

`apps/ios/RaptorKit` also builds and runs on macOS, so you can work on it without
the simulator:
`swift run raptor-snap terminal <server> out.png` renders a live terminal through
the iOS renderer, and `swift run raptor-snap views <server> dir/` renders the
screens offscreen.

**macOS** (macOS 15+, Xcode, `brew install xcodegen`):

```sh
scripts/build-apple.sh                # the macOS slice is always built
cd apps/macos && xcodegen generate && open RaptorMac.xcodeproj
```

The app is signed ad hoc, so it runs on the Mac that built it. Debug builds
accept automation commands (`dev.raptor.automation` distributed notifications,
see `Automation.swift`) so UI tests can drive a window without focusing it.

**Linux** (GTK 4.12+, libadwaita 1.5+, Pango 1.56+):

```sh
cargo run -p raptor-gtk --release
```

With Nix, `nix develop` gives a shell with GTK, libadwaita and Zig, and
`nix build` builds the app with its desktop entry, icon and fonts
(`nix profile install .` to install it).

Raptor finds the local Rex server's socket automatically. For a remote one, use
anything you'd pass to `rex -S`: `my-box` (port 7878), `100.64.0.2:7878`,
`box.tailnet.ts.net` (HTTPS via Rex's built-in Tailscale), or a socket path.

## Status of v0.1.0

| | Builds | Verified |
| --- | --- | --- |
| Rust core | ✅ | 68 unit and fake-server tests, plus 16 live end-to-end tests against a real Rex server: typing, keys, paste, resize and ownership, agent status, reconnect and byte-exact resume across network loss |
| Android | ✅ APK | Emulator against a real server: add server, sessions, typing and IME, key bar, hardware keys, background agent notifications with inline reply, network loss, rotation, tablet size, themes. Not yet run on a physical phone |
| iOS | ✅ device + Simulator builds | XCUITest against a real server on iPhone and iPad: typing, IME, hardware keyboard shortcuts, rotation, tiled panes, inbox replies, notification deep links, Dynamic Type. Not yet run on a physical iPhone |
| macOS | ✅ (ad hoc signed) | Against a real server: rendering, IME and dead keys, Option as Meta, selection, find, mouse reporting, splits and dividers, tabs, inbox replies, reconnect after a stalled server |
| Linux | ✅ (built on NixOS) | Headless sway against a real server: typing, keys and modifiers, mouse reporting (nvim), selection and copy/paste, tabs, splits and divider drags, reconnects, notifications, 1.5x scale |

## Roadmap

- v0.2: a Live Activity for agents, push notifications through a relay, and configurable key bars.
- v2: Raptor's own multiplexer server (Rust), speaking the same protocol.

## License

MIT. libghostty-rs and libghostty-vt are MIT-licensed by Mitchell Hashimoto and contributors.
JetBrains Mono and the other bundled fonts are under the SIL Open Font License;
Symbols Nerd Font is under the licenses in `assets/fonts/licenses`. Rex is a product of
Superlogical. Raptor is an independent project, not affiliated with or endorsed
by Superlogical.
