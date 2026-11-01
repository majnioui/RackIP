import Foundation

/// A macOS network service, as listed by `networksetup -listnetworkserviceorder`.
public struct NetworkService: Equatable, Hashable, Identifiable {
    public var id: String { name }
    public let name: String
    public let hardwarePort: String
    /// BSD device, e.g. "en5". Empty for VPNs and similar.
    public let device: String
    public let enabled: Bool

    public init(name: String, hardwarePort: String, device: String, enabled: Bool) {
        self.name = name
        self.hardwarePort = hardwarePort
        self.device = device
        self.enabled = enabled
    }

    /// A best guess at "this is a cable port": built-in Ethernet, USB and
    /// Thunderbolt adapters, docks. Wi-Fi, bridges, phones and VPNs are not.
    public var isWired: Bool {
        let port = hardwarePort.lowercased()
        let excluded = ["wi-fi", "wifi", "airport", "bluetooth", "bridge", "iphone", "ipad", "vpn", "firewire", "modem"]
        if excluded.contains(where: port.contains) { return false }
        guard device.hasPrefix("en") else { return false }
        return ["ethernet", "lan", "usb", "thunderbolt", "dock", "adapter", "nic", "gigabit"].contains(where: port.contains)
            || port.contains("10/100")
    }
}

public enum NetworkSetupParser {
    /// Parses `networksetup -listnetworkserviceorder`:
    ///
    ///     (1) USB 10/100/1000 LAN
    ///     (Hardware Port: USB 10/100/1000 LAN, Device: en5)
    ///
    /// Disabled services are numbered `(*)`.
    public static func services(fromServiceOrder output: String) -> [NetworkService] {
        var result: [NetworkService] = []
        var pending: (name: String, enabled: Bool)?
        for raw in output.split(separator: "\n", omittingEmptySubsequences: true) {
            let line = raw.trimmingWhitespace
            if line.hasPrefix("(Hardware Port:"), let p = pending {
                let body = line.dropFirst(1).dropLast(line.hasSuffix(")") ? 1 : 0)
                var port = "", device = ""
                if let devRange = body.range(of: ", Device:", options: .backwards) {
                    port = String(body[..<devRange.lowerBound]).replacingPrefix("Hardware Port:").trimmingWhitespace
                    device = String(body[devRange.upperBound...]).trimmingWhitespace
                }
                result.append(NetworkService(name: p.name, hardwarePort: port, device: device, enabled: p.enabled))
                pending = nil
            } else if line.hasPrefix("("), let close = line.firstIndex(of: ")") {
                let tag = line[line.index(after: line.startIndex)..<close]
                guard tag == "*" || Int(tag) != nil else { continue }
                let name = String(line[line.index(after: close)...]).trimmingWhitespace
                pending = (name, tag != "*")
            }
        }
        return result
    }

    /// Parses `networksetup -getinfo <service>`.
    public static func info(fromGetInfo output: String) -> ServiceInfo {
        var info = ServiceInfo()
        for raw in output.split(separator: "\n") {
            let line = raw.trimmingWhitespace
            if line.hasSuffix("Configuration") && !line.contains(":") {
                if line.hasPrefix("DHCP") { info.mode = .dhcp }
                else if line.hasPrefix("Manual") { info.mode = .manual }
                else if line.hasPrefix("BOOTP") { info.mode = .bootp }
                continue
            }
            guard let colon = line.firstIndex(of: ":") else { continue }
            let key = line[..<colon].trimmingWhitespace
            let value = String(line[line.index(after: colon)...]).trimmingWhitespace
            let address = IPv4(value)
            switch key {
            case "IP address": info.address = address
            case "Subnet mask": info.mask = address
            case "Router": info.router = address
            default: break
            }
        }
        return info
    }
}

public struct ServiceInfo: Equatable {
    public enum Mode: String { case dhcp = "DHCP", manual = "Manual", bootp = "BOOTP", unknown = "Off" }
    public var mode: Mode = .unknown
    public var address: IPv4?
    public var mask: IPv4?
    public var router: IPv4?
    public init() {}
}

extension String {
    func replacingPrefix(_ prefix: String) -> String {
        hasPrefix(prefix) ? String(dropFirst(prefix.count)) : self
    }
}
