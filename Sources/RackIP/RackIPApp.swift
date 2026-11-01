import SwiftUI

@main
struct RackIPApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        Window("RackIP", id: "main") {
            ContentView().environmentObject(model)
        }
        .windowResizability(.contentSize)

        // Quick switching without opening the window.
        MenuBarExtra {
            MenuBarContent().environmentObject(model)
        } label: {
            Image(nsImage: .ethernetPort)
        }
    }
}

struct MenuBarContent: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        if let s = model.selectedService {
            Text("\(s.name)\(model.isActive(s) ? " · connected" : "")")
            if let ip = model.currentInfo?.address {
                Text(verbatim: "\(model.currentInfo?.mode.rawValue ?? "") · \(ip)")
            }
        } else {
            Text("No port selected")
        }
        Divider()

        if model.profiles.isEmpty {
            Text("No saved addresses")
        } else {
            ForEach(model.sortedProfiles.prefix(15)) { p in
                Button(p.menuTitle) { model.apply(p) }
            }
        }
        Button("Use DHCP") { model.useDHCP() }
            .disabled(model.selectedService == nil)
        Divider()

        Button("Open RackIP…") {
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
        }
        .keyboardShortcut("o")
        Button("Quit RackIP") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }
}

extension NSImage {
    /// Menu bar glyph: the front of an RJ45 port. A template image, so macOS
    /// tints it for light and dark menu bars.
    static let ethernetPort: NSImage = {
        let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { _ in
            let housing = NSBezierPath(roundedRect: NSRect(x: 1.5, y: 2.5, width: 15, height: 13), xRadius: 2.5, yRadius: 2.5)
            housing.lineWidth = 1.5
            NSColor.black.set()
            housing.stroke()
            // Opening with latch slot.
            NSBezierPath(rect: NSRect(x: 4.5, y: 7.5, width: 9, height: 5.5)).fill()
            NSBezierPath(rect: NSRect(x: 7, y: 5, width: 4, height: 3)).fill()
            return true
        }
        image.isTemplate = true
        return image
    }()
}
