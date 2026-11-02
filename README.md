# Shrew

Open-source clients for [Rex](https://superlogical.com) terminal servers: native
apps for **iPhone/iPad**, **Android**, and **Linux** that connect to the Rex
server you already run, on your Mac or on any box on your tailnet.

> The shrew outlived the T. rex.

v0.1.0 is about frontends. They talk to an official Rex server (the Linux build
makes a good headless backend). Shrew's own multiplexer server is planned for v2.

## What's in it

- **Pixel-exact terminals.** Rex streams a Ghostty snapshot of each terminal and
  then raw output. Shrew restores it with
  [libghostty-vt](https://github.com/ghostty-org/ghostty), the same VT engine as
  Ghostty, so colors, wide characters, styles, and scrollback match byte for byte.
  Box-drawing characters are drawn as geometry, so lines join with no gaps.
- **"Needs you" inbox.** Programs that report status (OSC 7501, e.g. coding
  agents) show up at the top: blocked, working, done. You see the last lines of
  output and can reply without opening the terminal. Notifications fire when an
  agent needs you or finishes. On iOS you can reply straight from the
  notification.
- **Fit or mirror.** If nobody else holds a terminal's size, your device takes it
  and the terminal reflows to your screen. If your desktop holds it, Shrew mirrors
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
| `crates/shrew-proto` | Rex wire protocol: control API rev 8, data handshake, in-band messages, snapshot framing | Rust |
| `crates/shrew-client` | Async client: control WebSocket, resumable data streams, Unix/TCP/TLS endpoints | Rust |
| `crates/shrew-term` | libghostty-vt wrapper: snapshot restore, VT, frames as style runs, selection, scrollback | Rust + Zig |
| `crates/shrew-core` | The app engine every frontend shares: servers, reconnects, live state, agent status, terminals | Rust |
| `crates/shrew-ffi` | UniFFI bindings for Swift and Kotlin | Rust |
| `crates/shrew-cli` | Debug tool (`ls`, `dump`, `screen`) | Rust |
| `apps/android` | Android app: Jetpack Compose UI, Canvas terminal view | Kotlin |
| `apps/ios` | iOS/iPadOS app: SwiftUI, Core Text terminal, UIKit input (`ShrewKit` package + app target) | Swift |
| `apps/linux` | Linux app: GTK 4 + libadwaita, Cairo/Pango terminal, tiled panes | Rust |

The Rex protocol is documented by every Rex server at `GET /llms.txt`.

## Building

You need Rust (via rustup for cross-compiling), Zig 0.17 (to build
libghostty-vt), and the SDK for each platform.

```sh
git clone --recursive https://github.com/teddytennant/shrew
cargo test --workspace
SHREW_TEST_SERVER=my-box cargo test -p shrew-core --test live   # end-to-end against a real Rex server
```

**Android** (SDK, NDK, JDK 21, `cargo install cargo-ndk`):

```sh
scripts/build-android.sh              # Rust core for arm64-v8a + x86_64, plus Kotlin bindings
cd apps/android && ./gradlew :app:assembleDebug
```

**iOS** (Xcode, `brew install xcodegen`):

```sh
scripts/build-apple.sh                # ShrewFFI.xcframework + Swift bindings
cd apps/ios && xcodegen generate && open Shrew.xcodeproj
```

`apps/ios/ShrewKit` also builds and runs on macOS, so you can work on it without
the simulator:
`swift run shrew-snap terminal <server> out.png` renders a live terminal through
the iOS renderer, and `swift run shrew-snap views <server> dir/` renders the
screens offscreen.

**Linux** (GTK 4.12+, libadwaita 1.5+):

```sh
cargo run -p shrew-gtk --release
```

Shrew finds the local Rex server's socket automatically. For a remote one, use
anything you'd pass to `rex -S`: `my-box` (port 7878), `100.64.0.2:7878`,
`box.tailnet.ts.net` (HTTPS via Rex's built-in Tailscale), or a socket path.

## Status of v0.1.0

| | Builds | Verified |
| --- | --- | --- |
| Rust core | ✅ | 17 unit tests, plus a live end-to-end test against Rex on Linux |
| Android | ✅ APK | Run on an emulator against a real server: add server, sessions, typing, rendering |
| Linux | ✅ (built on NixOS) | Run headless (Broadway) against a real server |
| iOS | Needs Xcode for a device build | ShrewKit compiles on macOS. All iOS code type-checks against UIKit (Mac Catalyst SDK). Renderer and screens verified offscreen against a real server |

## Roadmap

- v0.2: iPad and tablet layouts showing every pane, a Live Activity for agents, push notifications through a relay, and configurable key bars and themes.
- v2: Shrew's own multiplexer server (Rust), speaking the same protocol.

## License

MIT. libghostty-vt is MIT-licensed by Mitchell Hashimoto and contributors.
JetBrains Mono is under the SIL Open Font License. Rex is a product of
Superlogical. Shrew is an independent project, not affiliated with or endorsed
by Superlogical.
