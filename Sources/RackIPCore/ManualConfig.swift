import Foundation

/// A validated static IPv4 configuration, ready to hand to the OS.
public struct ManualConfig: Equatable {
    public let address: IPv4
    public let mask: IPv4
    public let gateway: IPv4?
    /// Things that are allowed but probably a mistake.
    public let warnings: [String]

    public static let defaultMask = IPv4("255.255.255.0")!

    /// Builds a config from what the user typed.
    ///
    /// - `address` may carry a prefix ("10.0.0.5/24"); it then sets the mask
    ///   unless `mask` is also given, in which case the two must agree.
    /// - `mask` accepts dotted form or a prefix ("24" or "/24"). Empty means
    ///   the /24 default.
    /// - `gateway` is optional.
    public static func parse(address: String, mask: String, gateway: String) -> Result<ManualConfig, ConfigError> {
        let address = address.trimmingWhitespace
        let mask = mask.trimmingWhitespace
        let gateway = gateway.trimmingWhitespace

        guard !address.isEmpty else { return .failure(.missingAddress) }

        var addressText = Substring(address)
        var maskFromAddress: IPv4?
        if let slash = address.firstIndex(of: "/") {
            addressText = address[..<slash]
            let prefixText = address[address.index(after: slash)...]
            guard let prefix = Int(prefixText), let m = IPv4.mask(prefix: prefix) else {
                return .failure(.invalidMask(String(prefixText)))
            }
            maskFromAddress = m
        }
        guard let ip = IPv4(String(addressText)) else {
            return .failure(.invalidAddress(String(addressText)))
        }

        var netmask: IPv4
        if mask.isEmpty {
            netmask = maskFromAddress ?? defaultMask
        } else {
            guard let m = parseMask(mask) else { return .failure(.invalidMask(mask)) }
            if let fromAddress = maskFromAddress, fromAddress != m {
                return .failure(.conflictingMask(address: fromAddress, mask: m))
            }
            netmask = m
        }
        guard let prefix = netmask.prefixLength, prefix >= 1 else {
            return .failure(.invalidMask(netmask.description))
        }

        if ip.value == 0 || ip.value >> 24 == 127 || ip.value >> 28 >= 14 {
            return .failure(.notAHostAddress(ip))
        }
        if prefix <= 30 {
            let host = ip.value & ~netmask.value
            if host == 0 { return .failure(.networkAddress(ip, prefix: prefix)) }
            if host == ~netmask.value { return .failure(.broadcastAddress(ip, prefix: prefix)) }
        }

        var gw: IPv4?
        var warnings: [String] = []
        if !gateway.isEmpty {
            guard let g = IPv4(gateway) else { return .failure(.invalidGateway(gateway)) }
            if g == ip { return .failure(.gatewayIsAddress) }
            if g.network(mask: netmask) != ip.network(mask: netmask) {
                warnings.append("Gateway \(g) is outside \(IPv4(value: ip.network(mask: netmask)))/\(prefix), so it will not be reachable.")
            }
            gw = g
        }

        return .success(ManualConfig(address: ip, mask: netmask, gateway: gw, warnings: warnings))
    }

    static func parseMask(_ text: String) -> IPv4? {
        let bare = text.hasPrefix("/") ? String(text.dropFirst()) : text
        if let prefix = Int(bare) { return IPv4.mask(prefix: prefix) }
        guard let m = IPv4(text), m.prefixLength != nil else { return nil }
        return m
    }
}

public enum ConfigError: Error, Equatable, CustomStringConvertible {
    case missingAddress
    case invalidAddress(String)
    case invalidMask(String)
    case conflictingMask(address: IPv4, mask: IPv4)
    case invalidGateway(String)
    case gatewayIsAddress
    case notAHostAddress(IPv4)
    case networkAddress(IPv4, prefix: Int)
    case broadcastAddress(IPv4, prefix: Int)

    public var description: String {
        switch self {
        case .missingAddress: return "Enter an IP address."
        case .invalidAddress(let s): return "“\(s)” is not a valid IPv4 address."
        case .invalidMask(let s): return "“\(s)” is not a valid subnet mask."
        case let .conflictingMask(a, m): return "The /prefix on the address (\(a)) does not match the subnet mask (\(m))."
        case .invalidGateway(let s): return "“\(s)” is not a valid gateway address."
        case .gatewayIsAddress: return "The gateway cannot be the same as the IP address."
        case .notAHostAddress(let ip): return "\(ip) cannot be assigned to an interface."
        case let .networkAddress(ip, p): return "\(ip) is the network address of its /\(p) subnet."
        case let .broadcastAddress(ip, p): return "\(ip) is the broadcast address of its /\(p) subnet."
        }
    }
}

extension StringProtocol {
    var trimmingWhitespace: String { trimmingCharacters(in: .whitespacesAndNewlines) }
}
