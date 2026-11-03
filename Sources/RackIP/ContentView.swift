import SwiftUI
import RackIPCore

struct ContentView: View {
    enum Tab: String, CaseIterable, Identifiable {
        case staticIP = "Static IP"
        case dhcp = "DHCP Server"
        case about = "About"
        var id: Self { self }
    }

    @EnvironmentObject var model: AppModel
    @State private var tab: Tab = .staticIP

    var body: some View {
        VStack(spacing: 0) {
            Picker("View", selection: $tab) {
                ForEach(Tab.allCases) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .padding(.top, 14)

            switch tab {
            case .staticIP:
                Form {
                    portSection
                    addressSection
                }
                .formStyle(.grouped)

                footer
            case .dhcp:
                Form {
                    portSection
                    dhcpSection
                    if model.isDHCPRunning { devicesSection }
                }
                .formStyle(.grouped)

                dhcpFooter
            case .about:
                AboutView()
            }
        }
        .frame(width: 460)
        .fixedSize(horizontal: false, vertical: true)
    }

    // MARK: Port

    private var portSection: some View {
        Section {
            Picker("Port", selection: $model.selectedServiceName) {
                ForEach(model.visibleServices) { s in
                    Text(portLabel(s)).tag(Optional(s.name))
                }
            }
            .disabled(model.isDHCPRunning)
            .help(model.isDHCPRunning ? "Stop the DHCP server to switch ports" : "")
            LabeledContent("Current") {
                HStack(spacing: 6) {
                    if let s = model.selectedService {
                        Circle()
                            .fill(model.isActive(s) ? Color.green : Color.secondary.opacity(0.4))
                            .frame(width: 8, height: 8)
                            .help(model.isActive(s) ? "Cable connected" : "No link")
                    }
                    Text(currentDescription)
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
            Toggle("Show Wi-Fi and other ports", isOn: $model.showAllServices)
                .font(.callout)
                .foregroundStyle(.secondary)
        } header: {
            HStack {
                Text("Network port")
                Spacer()
                Button {
                    Task { await model.refresh() }
                } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless)
                    .help("Refresh")
            }
        }
    }

    private func portLabel(_ s: NetworkService) -> String {
        var label = s.name
        if !s.device.isEmpty { label += " (\(s.device))" }
        if model.isActive(s) { label += " · connected" }
        return label
    }

    private var currentDescription: String {
        guard model.selectedService != nil else { return "No port selected" }
        guard let info = model.currentInfo else { return "…" }
        guard let ip = info.address else { return "\(info.mode.rawValue) · no address" }
        var text = "\(info.mode.rawValue) · \(ip)"
        if let prefix = info.mask?.prefixLength { text += "/\(prefix)" }
        text += info.router.map { " · gw \($0)" } ?? " · no gateway"
        return text
    }

    // MARK: Address

    private var addressSection: some View {
        Section("Address") {
            HStack {
                Picker("Saved", selection: Binding(get: { model.selectedProfileID }, set: { model.selectProfile($0) })) {
                    Text("New address").tag(UUID?.none)
                    if !model.profiles.isEmpty { Divider() }
                    ForEach(model.sortedProfiles) { p in
                        Text(p.menuTitle).tag(Optional(p.id))
                    }
                }
                Button {
                    model.deleteSelectedProfile()
                } label: { Image(systemName: "trash") }
                    .buttonStyle(.borderless)
                    .disabled(model.selectedProfileID == nil)
                    .help("Delete this saved address")
            }

            TextField("Name", text: $model.name, prompt: Text(namePlaceholder))
            TextField("IP address", text: $model.address, prompt: Text("10.0.0.5 or 10.0.0.5/24"))
            TextField("Subnet mask", text: $model.mask, prompt: Text(maskPlaceholder))
            TextField("Gateway", text: $model.gateway, prompt: Text("Optional"))

            if let hint = formHint {
                Text(hint.text)
                    .font(.caption)
                    .foregroundStyle(hint.isError ? Color.red : Color.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    /// Unnamed addresses are saved under their IP.
    private var namePlaceholder: String {
        let ip = model.address.trimmingCharacters(in: .whitespaces)
        return ip.isEmpty ? "Optional, defaults to the IP" : ip
    }

    private var maskPlaceholder: String {
        if case .success(let c) = model.parsedForm, model.mask.isEmpty { return c.mask.description }
        return ManualConfig.defaultMask.description
    }

    /// Inline validation once something is typed; quiet while the field is empty.
    private var formHint: (text: String, isError: Bool)? {
        guard !model.address.isEmpty else {
            return ("Leave the gateway empty to keep internet on Wi-Fi while you work on the server network.", false)
        }
        switch model.parsedForm {
        case .failure(let e): return (e.description, true)
        case .success(let c):
            if let w = c.warnings.first { return (w, true) }
            if c.gateway == nil { return ("No gateway: only the local subnet is reached through this port.", false) }
            return nil
        }
    }

    // MARK: DHCP server

    private var dhcpSection: some View {
        Section("DHCP server") {
            let defaults = model.dhcpDefaults
            TextField("Server address", text: $model.dhcpServer, prompt: Text(DHCPConfig.defaultServer.description))
            TextField("Subnet mask", text: $model.dhcpMask, prompt: Text(defaults?.mask.description ?? ManualConfig.defaultMask.description))
            TextField("Range start", text: $model.dhcpRangeStart, prompt: Text(defaults?.rangeStart.description ?? ""))
            TextField("Range end", text: $model.dhcpRangeEnd, prompt: Text(defaults?.rangeEnd.description ?? ""))
            TextField("Lease (minutes)", text: $model.dhcpLeaseMinutes, prompt: Text(String(DHCPConfig.defaultLeaseMinutes)))

            Group {
                switch model.parsedDHCP {
                case .failure(let e):
                    Text(e.description).foregroundStyle(.red)
                case .success(let c):
                    Text(verbatim: "The port gets \(c.server)/\(c.prefix). Devices on the cable get \(c.rangeStart) to \(c.rangeEnd). No gateway or DNS is handed out, so they only reach this Mac.")
                        .foregroundStyle(.secondary)
                }
            }
            .font(.caption)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .disabled(model.isDHCPRunning)
    }

    private var devicesSection: some View {
        Section("Devices") {
            if model.dhcpLeases.isEmpty {
                Text("Waiting for devices. Plug one in, or restart its network so it asks for an address.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            ForEach(model.dhcpLeases) { lease in
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(lease.ip.description)
                            .font(.body.monospacedDigit())
                        Text([lease.hostname ?? lease.vendor, lease.mac].compactMap { $0 }.joined(separator: " · "))
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .textSelection(.enabled)
                    Spacer()
                    Text("until \(lease.expires, style: .time)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var dhcpFooter: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Spacer()
                if model.isBusy { ProgressView().controlSize(.small) }
                if model.isDHCPRunning {
                    Button("Stop") { model.stopDHCP() }
                } else {
                    Button("Start") { model.startDHCP() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(model.isBusy || model.selectedService == nil)
                }
            }
            statusLabel
        }
        .padding(.horizontal, 20)
        .padding(.bottom, 16)
    }

    // MARK: Footer

    private var footer: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Button("Use DHCP") { model.useDHCP() }
                    .help("Switch this port back to automatic addressing")
                Spacer()
                if model.isBusy { ProgressView().controlSize(.small) }
                Button("Save") { model.saveProfile() }
                    .help("Save this address without applying it")
                Button("Apply") { model.applyForm() }
                    .keyboardShortcut(.defaultAction)
            }
            .disabled(model.isBusy || model.selectedService == nil)

            statusLabel
        }
        .padding(.horizontal, 20)
        .padding(.bottom, 16)
    }

    @ViewBuilder
    private var statusLabel: some View {
        if let status = model.status {
            Label(status.text, systemImage: icon(status.kind))
                .font(.callout)
                .foregroundStyle(color(status.kind))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func icon(_ kind: AppModel.Status.Kind) -> String {
        switch kind {
        case .info: return "clock"
        case .success: return "checkmark.circle.fill"
        case .warning: return "exclamationmark.triangle.fill"
        case .error: return "xmark.octagon.fill"
        }
    }

    private func color(_ kind: AppModel.Status.Kind) -> Color {
        switch kind {
        case .info: return .secondary
        case .success: return .green
        case .warning: return .orange
        case .error: return .red
        }
    }
}
