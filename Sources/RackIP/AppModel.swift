import Foundation
import RackIPCore

@MainActor
final class AppModel: ObservableObject {
    struct Status: Equatable {
        enum Kind { case info, success, warning, error }
        var kind: Kind
        var text: String
    }

    // Ports
    @Published private(set) var services: [NetworkService] = []
    @Published private(set) var activeDevices: Set<String> = []
    @Published var selectedServiceName: String? {
        didSet {
            UserDefaults.standard.set(selectedServiceName, forKey: "selectedService")
            if oldValue != selectedServiceName { currentInfo = nil; Task { await refresh() } }
        }
    }
    @Published private(set) var currentInfo: ServiceInfo?
    @Published var showAllServices = UserDefaults.standard.bool(forKey: "showAllServices") {
        didSet { UserDefaults.standard.set(showAllServices, forKey: "showAllServices") }
    }

    // Form
    @Published var name = ""
    @Published var address = ""
    @Published var mask = ""
    @Published var gateway = ""

    // Profiles
    @Published private(set) var profiles: [Profile] = ProfileStore.load()
    @Published private(set) var selectedProfileID: UUID?

    @Published var status: Status?
    @Published private(set) var isBusy = false

    private var timer: Timer?

    init() {
        selectedServiceName = UserDefaults.standard.string(forKey: "selectedService")
        Task { await refresh() }
        // Keep the link indicator live while cables get swapped.
        timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in
            Task { @MainActor in await self?.refresh() }
        }
    }

    // MARK: Derived

    var visibleServices: [NetworkService] {
        let wired = services.filter(\.isWired)
        if showAllServices || wired.isEmpty { return services.filter { !$0.device.isEmpty } }
        // Never hide the one that's selected.
        return services.filter { $0.isWired || $0.name == selectedServiceName }
    }

    var selectedService: NetworkService? { services.first { $0.name == selectedServiceName } }

    func isActive(_ service: NetworkService) -> Bool { activeDevices.contains(service.device) }

    var sortedProfiles: [Profile] {
        profiles.sorted { ($0.lastUsed ?? .distantPast, $1.displayName) > ($1.lastUsed ?? .distantPast, $0.displayName) }
    }

    /// Live preview of what Apply would do, for the form's hint line.
    var parsedForm: Result<ManualConfig, ConfigError> {
        ManualConfig.parse(address: address, mask: mask, gateway: gateway)
    }

    // MARK: Refresh

    func refresh() async {
        let selected = selectedServiceName
        let result = await Task.detached { () -> ([NetworkService], Set<String>, ServiceInfo?)? in
            guard let services = try? NetworkSetup.services() else { return nil }
            let active = Set(services.map(\.device).filter { NetworkSetup.isLinkActive(device: $0) })
            let info = selected.flatMap { name in services.contains { $0.name == name } ? try? NetworkSetup.info(service: name) : nil }
            return (services, active, info)
        }.value
        guard let (services, active, info) = result else { return }
        if self.services != services { self.services = services }
        if activeDevices != active { activeDevices = active }
        if selectedServiceName == selected, currentInfo != info { currentInfo = info }

        if selectedService == nil {
            // First launch or the adapter was unplugged: prefer a wired port with link.
            let wired = services.filter(\.isWired)
            selectedServiceName = (wired.first { active.contains($0.device) } ?? wired.first ?? services.first)?.name
        }
    }

    // MARK: Profiles

    func selectProfile(_ id: UUID?) {
        selectedProfileID = id
        guard let id, let p = profiles.first(where: { $0.id == id }) else {
            name = ""; address = ""; mask = ""; gateway = ""
            return
        }
        // Leave the name field empty when it is just the IP, so editing the IP renames it too.
        name = p.isNamedAfterAddress ? "" : p.name; address = p.address; mask = p.mask; gateway = p.gateway
        if let svc = p.service, services.contains(where: { $0.name == svc }) {
            selectedServiceName = svc
        }
    }

    func deleteSelectedProfile() {
        guard let id = selectedProfileID else { return }
        profiles.removeAll { $0.id == id }
        ProfileStore.save(profiles)
        selectProfile(nil)
    }

    /// Saves the form as a profile without touching the network.
    func saveProfile() {
        switch parsedForm {
        case .failure(let e): status = Status(kind: .error, text: e.description)
        case .success:
            let p = upsertProfile(markUsed: false)
            status = Status(kind: .success, text: "Saved “\(p.displayName)”.")
        }
    }

    @discardableResult
    private func upsertProfile(markUsed: Bool) -> Profile {
        let trim = { (s: String) in s.trimmingCharacters(in: .whitespacesAndNewlines) }
        var p = profiles.first { $0.id == selectedProfileID }
            ?? profiles.first { trim($0.address) == trim(address) && trim($0.mask) == trim(mask) && trim($0.gateway) == trim(gateway) }
            ?? Profile(name: "", address: "", mask: "", gateway: "")
        let typedName = trim(name)
        // Retyping a saved IP with no name keeps that address's existing name.
        let keepName = typedName.isEmpty && p.id != selectedProfileID && !p.isNamedAfterAddress
        if !keepName { p.name = typedName.isEmpty ? trim(address) : typedName }
        p.address = trim(address); p.mask = trim(mask); p.gateway = trim(gateway)
        if markUsed { p.service = selectedServiceName; p.lastUsed = Date() }
        if let i = profiles.firstIndex(where: { $0.id == p.id }) { profiles[i] = p } else { profiles.append(p) }
        ProfileStore.save(profiles)
        selectedProfileID = p.id
        return p
    }

    // MARK: Apply

    func applyForm() {
        guard let service = selectedServiceName else {
            status = Status(kind: .error, text: "Choose a network port first.")
            return
        }
        let config: ManualConfig
        switch parsedForm {
        case .failure(let e): status = Status(kind: .error, text: e.description); return
        case .success(let c): config = c
        }
        upsertProfile(markUsed: true)
        perform("Setting \(config.address) on \(service)…", warnings: config.warnings) {
            try NetworkSetup.setManual(service: service, config: config)
        } success: {
            var text = "\(service) is now \(config.address)/\(config.mask.prefixLength ?? 0)"
            if let gw = config.gateway { text += " via \(gw)" }
            return text + "."
        }
    }

    /// Menu bar quick-apply.
    func apply(_ profile: Profile) {
        if let svc = profile.service, services.contains(where: { $0.name == svc }) {
            selectedServiceName = svc
        }
        selectProfile(profile.id)
        applyForm()
    }

    func useDHCP() {
        guard let service = selectedServiceName else { return }
        perform("Switching \(service) to DHCP…", warnings: []) {
            try NetworkSetup.setDHCP(service: service)
        } success: { "\(service) is back on DHCP (automatic)." }
    }

    private func perform(_ progress: String, warnings: [String],
                         _ work: @escaping @Sendable () throws -> Void,
                         success: @escaping () -> String) {
        guard !isBusy else { return }
        isBusy = true
        status = Status(kind: .info, text: progress)
        Task {
            let error = await Task.detached { () -> String? in
                do { try work(); return nil } catch { return error.localizedDescription }
            }.value
            isBusy = false
            if let error {
                status = Status(kind: .error, text: error)
            } else if let w = warnings.first {
                status = Status(kind: .warning, text: success() + " " + w)
            } else {
                status = Status(kind: .success, text: success())
            }
            // The system takes a moment to publish the new address.
            try? await Task.sleep(nanoseconds: 800_000_000)
            await refresh()
        }
    }
}
