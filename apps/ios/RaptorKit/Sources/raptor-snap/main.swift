// raptor-snap: render RaptorKit screens and live terminals to PNG files, offscreen.
//
//   swift run raptor-snap terminal <server> <out.png>   # throwaway session, demo output
//   swift run raptor-snap views <server> <outdir>       # SwiftUI screens via ImageRenderer
import AppKit
import Foundation
import RaptorKit
import SwiftUI

@MainActor
func spin(_ seconds: Double, until done: () -> Bool = { false }) {
    let end = Date().addingTimeInterval(seconds)
    while Date() < end, !done() {
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
    }
}

func writePNG(_ image: CGImage, to path: String) {
    let rep = NSBitmapImageRep(cgImage: image)
    try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: path))
    print("wrote \(path)")
}

@MainActor
func connect(_ address: String) -> (RaptorModel, String) {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("raptor-snap-\(getpid())")
    let model = RaptorModel(dataDirectory: dir, platform: "macos", deviceLabel: "raptor-snap")
    let id = try! model.core.addServer(name: "", address: address)
    spin(15) { model.server(id)?.connectivity.isOnline == true }
    guard model.server(id)?.connectivity.isOnline == true else {
        fatalError("could not connect: \(String(describing: model.server(id)?.connectivity))")
    }
    spin(1.5)
    return (model, id)
}

@MainActor
func withSession<T>(_ model: RaptorModel, _ server: String, _ body: (String, String) -> T) -> T {
    var sid: String?
    Task { sid = try? await model.core.createSession(serverId: server, label: "raptor-snap") }
    spin(10) { sid != nil }
    guard let sid else { fatalError("could not create a session") }
    spin(10) { model.session(server, sid)?.focusedBlock != nil }
    let block = model.session(server, sid)!.focusedBlock!.id
    defer {
        var done = false
        Task { try? await model.core.destroySession(serverId: server, sessionId: sid); done = true }
        spin(5) { done }
    }
    return body(sid, block)
}

let demo = #"""
printf '\033[1mRaptor on iOS\033[0m, rendered by Core Text\n\n'; printf '\033[1;35mbold\033[0m \033[3mitalic\033[0m \033[4:3mcurly\033[0m \033[9mstrike\033[0m \033[2mfaint\033[0m \033[7m inverse \033[0m\n'; printf 'wide: 中文字 🦔 ok\n'; printf '┌──────┬──────┐\n│ box  │ draw │\n├──────┼──────┤\n│ ▁▂▃▄ │ ▅▆▇█ │\n└──────┴──────┘\n'; printf '\033[30;44m main \033[34;42m\033[30m +3 \033[0;32m\033[0m   \n'; i=0; while [ $i -lt 16 ]; do printf '\033[38;2;%d;%d;255m█' $((i * 16)) $((255 - i * 16)); i=$((i + 1)); done; printf '\033[0m\n'; ls -la /etc | head -8
"""#

@MainActor
func snapTerminal(_ address: String, _ out: String) {
    let (model, server) = connect(address)
    withSession(model, server) { sid, block in
        let tc = TerminalController(model: model, serverId: server, sessionId: sid, blockId: block)
        let renderer = TerminalRenderer(fontSize: 13)
        // iPhone 17-ish portrait viewport in points.
        let size = CGSize(width: 402, height: 560)
        let grid = renderer.grid(for: size, padding: 6)
        tc.terminal?.setDeviceGrid(cols: UInt16(grid.cols), rows: UInt16(grid.rows), cellWidthPx: UInt32(renderer.metrics.width * 3), cellHeightPx: UInt32(renderer.metrics.height * 3))
        tc.takeControl()
        spin(10) { tc.status == .live }
        spin(1.5)
        // The demo is POSIX sh, whatever the server's login shell is.
        tc.terminal?.sendText(text: "exec sh\r")
        spin(1)
        tc.terminal?.sendText(text: "clear; " + demo)
        tc.terminal?.sendKey(key: .enter, ctrl: false, alt: false, shift: false, meta: false)
        spin(3)
        guard let frame = tc.frame() else { fatalError("no frame") }
        print("frame \(frame.cols)x\(frame.rows), status \(tc.status)")
        let scale: CGFloat = 3
        let ctx = CGContext(data: nil, width: Int(size.width * scale), height: Int(size.height * scale), bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        // Flip to UIKit's top-left origin.
        ctx.translateBy(x: 0, y: size.height * scale)
        ctx.scaleBy(x: scale, y: -scale)
        renderer.draw(frame, in: ctx, size: size, options: .init())
        writePNG(ctx.makeImage()!, to: out)
        tc.close()
    }
}

@MainActor
func snapViews(_ address: String, _ outDir: String) {
    let (model, server) = connect(address)
    try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)
    func render<V: View>(_ name: String, _ view: V, height: CGFloat = 874) {
        let r = ImageRenderer(content: view.frame(width: 402, height: height).environment(model))
        r.scale = 2
        if let img = r.cgImage { writePNG(img, to: "\(outDir)/\(name).png") }
    }
    withSession(model, server) { _, _ in
        spin(1)
        func home(_ scheme: ColorScheme) -> some View {
            VStack(alignment: .leading, spacing: 0) {
                HStack { Spacer(); Image(systemName: "plus").font(.title3).foregroundStyle(Palette.accent) }.padding(.horizontal, 20).padding(.top, 58)
                Text("Raptor").font(.largeTitle.bold()).padding(.horizontal, 20).padding(.top, 4)
                HomeList(onOpen: { _, _, _ in }, onAdd: {}, onEdit: { _ in }).padding(.horizontal, 16)
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .background(scheme == .dark ? Color.black : Color(white: 0.95))
            .environment(\.colorScheme, scheme)
        }
        render("home-dark", home(.dark))
        render("home-light", home(.light))
        render("add-server", AddServerView(editing: nil, onDone: {}).environment(\.colorScheme, .dark).background(Color.black), height: 600)
    }
    _ = server
}

/// The app icon: a prompt chevron and a raptor, on deep ink. Same art as Android's adaptive icon.
func drawIcon(size: Int, to path: String) {
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
    let s = CGFloat(size)
    // Background gradient.
    let grad = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB), colors: [CGColor(srgbRed: 0.09, green: 0.12, blue: 0.17, alpha: 1), CGColor(srgbRed: 0.04, green: 0.05, blue: 0.08, alpha: 1)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 0, y: s), end: CGPoint(x: s, y: 0), options: [])
    // Art is authored in a 108-unit box (flipped y), centered.
    ctx.translateBy(x: 0, y: s)
    ctx.scaleBy(x: s / 108, y: -s / 108)
    func fill(_ hex: UInt32, _ build: (CGMutablePath) -> Void) {
        let p = CGMutablePath(); build(p)
        ctx.addPath(p)
        ctx.setFillColor(CGColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255, green: CGFloat((hex >> 8) & 0xFF) / 255, blue: CGFloat(hex & 0xFF) / 255, alpha: 1))
        ctx.fillPath()
    }
    fill(0x34D399) { p in
        p.move(to: CGPoint(x: 33, y: 40)); p.addLine(to: CGPoint(x: 47, y: 54)); p.addLine(to: CGPoint(x: 33, y: 68))
        p.addLine(to: CGPoint(x: 38, y: 73)); p.addLine(to: CGPoint(x: 57, y: 54)); p.addLine(to: CGPoint(x: 38, y: 35)); p.closeSubpath()
    }
    fill(0xF3F4F6) { p in
        p.move(to: CGPoint(x: 58, y: 66))
        p.addCurve(to: CGPoint(x: 72, y: 52), control1: CGPoint(x: 58, y: 58), control2: CGPoint(x: 64, y: 52))
        p.addCurve(to: CGPoint(x: 82, y: 56), control1: CGPoint(x: 76, y: 52), control2: CGPoint(x: 79, y: 53))
        p.addLine(to: CGPoint(x: 88, y: 59))
        p.addCurve(to: CGPoint(x: 88, y: 62), control1: CGPoint(x: 89, y: 60), control2: CGPoint(x: 89, y: 61))
        p.addLine(to: CGPoint(x: 83, y: 64))
        p.addCurve(to: CGPoint(x: 71, y: 73), control1: CGPoint(x: 82, y: 69), control2: CGPoint(x: 77, y: 73))
        p.addLine(to: CGPoint(x: 62, y: 73))
        p.addCurve(to: CGPoint(x: 58, y: 69), control1: CGPoint(x: 60, y: 73), control2: CGPoint(x: 58, y: 71))
        p.closeSubpath()
    }
    fill(0xF9A8D4) { p in
        p.move(to: CGPoint(x: 68, y: 52))
        p.addCurve(to: CGPoint(x: 71, y: 47), control1: CGPoint(x: 67, y: 48), control2: CGPoint(x: 69, y: 46))
        p.addCurve(to: CGPoint(x: 71, y: 52), control1: CGPoint(x: 72, y: 48), control2: CGPoint(x: 72, y: 50))
        p.closeSubpath()
    }
    fill(0x111827) { p in p.addEllipse(in: CGRect(x: 74.4, y: 56.4, width: 3.2, height: 3.2)) }
    writePNG(ctx.makeImage()!, to: path)
}

let args = CommandLine.arguments
if args.count == 3, args[1] == "icon" {
    drawIcon(size: 1024, to: args[2])
    exit(0)
}
guard args.count >= 4 else {
    print("usage: raptor-snap terminal|views <server> <out>")
    exit(2)
}
MainActor.assumeIsolated {
    switch args[1] {
    case "terminal": snapTerminal(args[2], args[3])
    case "views": snapViews(args[2], args[3])
    default: print("unknown command"); exit(2)
    }
}
exit(0)
