import Foundation

/// A saved address, e.g. "Dell iDRAC – rack 12". Stored as text exactly as
/// typed so the form shows it back unchanged.
struct Profile: Codable, Identifiable, Hashable {
    var id = UUID()
    var name: String
    var address: String
    var mask: String
    var gateway: String
    /// The network service it was last applied to, so picking the profile
    /// also picks the port.
    var service: String?
    var lastUsed: Date?

    var displayName: String { name.isEmpty ? address : name }

    /// Unnamed addresses are named after the IP, so show it only once.
    var isNamedAfterAddress: Bool { name.isEmpty || name == address }

    var menuTitle: String { isNamedAfterAddress ? address : "\(name) — \(address)" }
}

enum ProfileStore {
    private static let key = "profiles"

    static func load() -> [Profile] {
        guard let data = UserDefaults.standard.data(forKey: key),
              let profiles = try? JSONDecoder().decode([Profile].self, from: data) else { return [] }
        return profiles
    }

    static func save(_ profiles: [Profile]) {
        if let data = try? JSONEncoder().encode(profiles) {
            UserDefaults.standard.set(data, forKey: key)
        }
    }
}
