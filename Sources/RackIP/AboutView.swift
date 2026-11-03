import SwiftUI

struct AboutView: View {
    private var version: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "dev"
    }

    var body: some View {
        VStack(spacing: 6) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 96, height: 96)
            Text("RackIP")
                .font(.title2.bold())
            Text("Version \(version)")
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
            Text("Switch your Ethernet port's IP address in one click.")
                .multilineTextAlignment(.center)
                .padding(.top, 6)

            VStack(spacing: 4) {
                Text("Made by Mo Majnioui")
                Link("majnioui.me", destination: URL(string: "https://majnioui.me")!)
                Link("Source on GitHub", destination: URL(string: "https://github.com/majnioui/RackIP")!)
            }
            .padding(.top, 14)

            Text("Free and open source, MIT License")
                .font(.caption)
                .foregroundStyle(.secondary)
                .padding(.top, 14)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 28)
        .padding(.horizontal, 20)
    }
}
