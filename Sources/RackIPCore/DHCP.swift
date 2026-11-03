import Foundation

/// What the DHCP server hands out: the port's own address, and a range for
/// the devices on the cable. No gateway or DNS, so devices only reach the laptop.
public struct DHCPConfig: Equatable {
    public let server: IPv4
    public let mask: IPv4
    public let rangeStart: IPv4
    public let rangeEnd: IPv4
    public let leaseSeconds: UInt32

    public static let defaultServer = IPv4("192.168.100.1")!
    public static let defaultLeaseMinutes = 60

    public var prefix: Int { mask.prefixLength ?? 0 }

    /// .100 to .200 when the subnet is big enough, otherwise every host address.
    public static func defaultRange(server: IPv4, mask: IPv4) -> (IPv4, IPv4) {
        let network = server.network(mask: mask)
        let size = UInt64(~mask.value) + 1
        if size >= 256 { return (IPv4(value: network + 100), IPv4(value: network + 200)) }
        return (IPv4(value: network + 1), IPv4(value: network + UInt32(size) - 2))
    }

    /// Empty fields fall back to the defaults shown as placeholders.
    public static func parse(server: String, mask: String, rangeStart: String, rangeEnd: String, leaseMinutes: String) -> Result<DHCPConfig, DHCPConfigError> {
        let serverText = server.trimmingWhitespace.isEmpty ? defaultServer.description : server
        let manual: ManualConfig
        switch ManualConfig.parse(address: serverText, mask: mask, gateway: "") {
        case .failure(let e): return .failure(DHCPConfigError(e.description))
        case .success(let c): manual = c
        }
        guard let prefix = manual.mask.prefixLength, prefix <= 30 else {
            return .failure("The subnet is too small to hand out addresses. Use /30 or larger.")
        }
        let (defaultStart, defaultEnd) = defaultRange(server: manual.address, mask: manual.mask)
        func address(_ text: String, _ fallback: IPv4, _ label: String) -> Result<IPv4, DHCPConfigError> {
            let t = text.trimmingWhitespace
            if t.isEmpty { return .success(fallback) }
            guard let ip = IPv4(t) else { return .failure("“\(t)” is not a valid \(label) address.") }
            return .success(ip)
        }
        let start: IPv4, end: IPv4
        switch address(rangeStart, defaultStart, "range start") {
        case .failure(let e): return .failure(e)
        case .success(let ip): start = ip
        }
        switch address(rangeEnd, defaultEnd, "range end") {
        case .failure(let e): return .failure(e)
        case .success(let ip): end = ip
        }
        let network = manual.address.network(mask: manual.mask)
        let broadcast = network | ~manual.mask.value
        let subnet = "\(IPv4(value: network))/\(prefix)"
        for ip in [start, end] where ip.network(mask: manual.mask) != network || ip.value == network || ip.value == broadcast {
            return .failure("\(ip) is not a host address in \(subnet).")
        }
        guard start.value <= end.value else { return .failure("The range starts after it ends.") }
        if start == end && start == manual.address {
            return .failure("The range only holds the server's own address.")
        }

        let minutesText = leaseMinutes.trimmingWhitespace
        let minutes = minutesText.isEmpty ? defaultLeaseMinutes : Int(minutesText) ?? -1
        guard (1...10080).contains(minutes) else { return .failure("Lease time is in minutes, from 1 to 10080 (a week).") }

        return .success(DHCPConfig(server: manual.address, mask: manual.mask, rangeStart: start, rangeEnd: end, leaseSeconds: UInt32(minutes * 60)))
    }

    public init(server: IPv4, mask: IPv4, rangeStart: IPv4, rangeEnd: IPv4, leaseSeconds: UInt32) {
        self.server = server
        self.mask = mask
        self.rangeStart = rangeStart
        self.rangeEnd = rangeEnd
        self.leaseSeconds = leaseSeconds
    }

    func inRange(_ ip: IPv4) -> Bool {
        ip.value >= rangeStart.value && ip.value <= rangeEnd.value && ip != server
    }
}

public struct DHCPConfigError: Error, Equatable, CustomStringConvertible, ExpressibleByStringInterpolation {
    public let description: String
    public init(_ description: String) { self.description = description }
    public init(stringLiteral value: String) { description = value }
}

/// An address handed to a device.
public struct DHCPLease: Equatable, Identifiable {
    public var id: String { mac }
    /// "aa:bb:cc:dd:ee:ff". Empty for an address a device reported as taken.
    public var mac: String
    public var ip: IPv4
    public var hostname: String?
    public var vendor: String?
    public var expires: Date
    /// False while only offered, true once the device accepted it.
    public var bound: Bool
}

/// The DHCP protocol (RFC 2131) without sockets: packet in, packet out.
public struct DHCPEngine {
    public enum MessageType: UInt8 {
        case discover = 1, offer, request, decline, ack, nak, release, inform
    }

    public static let broadcast = IPv4(value: .max)

    public let config: DHCPConfig
    public private(set) var leases: [DHCPLease] = []
    /// Another DHCP server seen on the cable. The engine goes quiet once set.
    public private(set) var foreignServer: IPv4?

    public init(config: DHCPConfig) { self.config = config }

    /// Devices holding an address now, newest first.
    public func activeLeases(now: Date) -> [DHCPLease] {
        leases.filter { $0.bound && !$0.mac.isEmpty && $0.expires > now }.sorted { $0.expires > $1.expires }
    }

    /// Handles one packet received on port 67. Returns the reply and where to
    /// send it (port 68), or nil when there is nothing to answer.
    public mutating func handle(_ packet: [UInt8], now: Date) -> (reply: [UInt8], to: IPv4)? {
        guard foreignServer == nil, let request = Request(packet) else { return nil }
        // Relayed requests come from other networks; this server is only for the cable.
        guard request.giaddr == 0 else { return nil }

        switch request.type {
        case .discover:
            guard let ip = pick(for: request, now: now) else { return nil }
            record(request, ip: ip, bound: false, expires: now.addingTimeInterval(60), now: now)
            return reply(to: request, type: .offer, yiaddr: ip)

        case .request:
            if let chosen = request.serverID, chosen != config.server {
                // The device took another server's offer.
                foreignServer = chosen
                leases.removeAll { $0.mac == request.mac && !$0.bound }
                return nil
            }
            guard let wanted = request.requestedIP ?? (request.ciaddr != 0 ? IPv4(value: request.ciaddr) : nil) else { return nil }
            if config.inRange(wanted) && !isHeld(wanted, byOtherThan: request.mac, now: now) {
                record(request, ip: wanted, bound: true, expires: now.addingTimeInterval(TimeInterval(config.leaseSeconds)), now: now)
                return reply(to: request, type: .ack, yiaddr: wanted)
            }
            // Refuse so the device starts over and gets an address from the range.
            return reply(to: request, type: .nak, yiaddr: IPv4(value: 0))

        case .decline:
            // Something else on the cable already uses it; keep it out of the pool for a while.
            guard let ip = request.requestedIP else { return nil }
            leases.removeAll { $0.ip == ip }
            leases.append(DHCPLease(mac: "", ip: ip, hostname: nil, vendor: nil, expires: now.addingTimeInterval(600), bound: false))
            return nil

        case .release:
            if let i = leases.firstIndex(where: { $0.mac == request.mac && $0.ip.value == request.ciaddr }) {
                leases[i].expires = now
                leases[i].bound = false
            }
            return nil

        case .inform:
            guard request.ciaddr != 0 else { return nil }
            return reply(to: request, type: .ack, yiaddr: IPv4(value: 0))

        case .offer, .ack, .nak:
            return nil
        }
    }

    // MARK: Pool

    private func isHeld(_ ip: IPv4, byOtherThan mac: String, now: Date) -> Bool {
        leases.contains { $0.ip == ip && $0.mac != mac && $0.expires > now }
    }

    private func pick(for request: Request, now: Date) -> IPv4? {
        if let previous = leases.first(where: { $0.mac == request.mac }), config.inRange(previous.ip),
           !isHeld(previous.ip, byOtherThan: request.mac, now: now) {
            return previous.ip
        }
        if let wanted = request.requestedIP, config.inRange(wanted), !isHeld(wanted, byOtherThan: request.mac, now: now) {
            return wanted
        }
        let range = config.rangeStart.value...config.rangeEnd.value
        // Never-used addresses first, so a device that comes back finds its old one free.
        if let fresh = range.lazy.map(IPv4.init(value:)).first(where: { ip in config.inRange(ip) && !leases.contains { $0.ip == ip } }) {
            return fresh
        }
        return range.lazy.map(IPv4.init(value:)).first { config.inRange($0) && !isHeld($0, byOtherThan: request.mac, now: now) }
    }

    private mutating func record(_ request: Request, ip: IPv4, bound: Bool, expires: Date, now: Date) {
        let previous = leases.first { $0.mac == request.mac }
        leases.removeAll { $0.mac == request.mac || ($0.ip == ip && $0.expires <= now) }
        var lease = DHCPLease(mac: request.mac, ip: ip, hostname: request.hostname ?? previous?.hostname,
                              vendor: request.vendor ?? previous?.vendor, expires: expires, bound: bound)
        // A DISCOVER from a device that still holds its lease does not shorten it.
        if !bound, let previous, previous.bound, previous.ip == ip, previous.expires > expires {
            lease.bound = true
            lease.expires = previous.expires
        }
        leases.append(lease)
    }

    // MARK: Packets

    private func reply(to request: Request, type: MessageType, yiaddr: IPv4) -> (reply: [UInt8], to: IPv4) {
        var p = [UInt8](repeating: 0, count: 236)
        p[0] = 2 // BOOTREPLY
        p[1] = request.raw[1]
        p[2] = request.raw[2]
        p.replaceSubrange(4..<8, with: request.raw[4..<8]) // xid
        p.replaceSubrange(10..<12, with: request.raw[10..<12]) // flags
        if type != .offer && type != .nak { put(request.ciaddr, into: &p, at: 12) }
        put(yiaddr.value, into: &p, at: 16)
        p.replaceSubrange(28..<44, with: request.raw[28..<44]) // chaddr
        p += [99, 130, 83, 99]

        func option(_ code: UInt8, _ data: [UInt8]) { p += [code, UInt8(data.count)] + data }
        func be32(_ v: UInt32) -> [UInt8] { [24, 16, 8, 0].map { UInt8(truncatingIfNeeded: v >> UInt32($0)) } }
        option(53, [type.rawValue])
        option(54, be32(config.server.value))
        if type == .offer || type == .ack {
            if type == .offer || yiaddr.value != 0 {
                option(51, be32(config.leaseSeconds))
                option(58, be32(config.leaseSeconds / 2))
                option(59, be32(config.leaseSeconds / 8 * 7))
            }
            option(1, be32(config.mask.value))
        }
        p.append(255)
        if p.count < 300 { p += [UInt8](repeating: 0, count: 300 - p.count) }

        // Devices without an address yet can only be reached by broadcast.
        let to = type != .nak && request.ciaddr != 0 ? IPv4(value: request.ciaddr) : DHCPEngine.broadcast
        return (p, to)
    }

    private func put(_ v: UInt32, into p: inout [UInt8], at i: Int) {
        for k in 0..<4 { p[i + k] = UInt8(truncatingIfNeeded: v >> UInt32(24 - 8 * k)) }
    }

    struct Request {
        let raw: [UInt8]
        let type: MessageType
        let ciaddr: UInt32
        let giaddr: UInt32
        let mac: String
        let requestedIP: IPv4?
        let serverID: IPv4?
        let hostname: String?
        let vendor: String?

        init?(_ p: [UInt8]) {
            guard p.count >= 240, p[0] == 1, p[236...239].elementsEqual([99, 130, 83, 99]) else { return nil }
            func be32(_ i: Int) -> UInt32 { p[i..<i + 4].reduce(0) { $0 << 8 | UInt32($1) } }
            var options: [UInt8: [UInt8]] = [:]
            var i = 240
            while i < p.count {
                let code = p[i]
                if code == 255 { break }
                if code == 0 { i += 1; continue }
                guard i + 1 < p.count else { break }
                let len = Int(p[i + 1])
                guard i + 2 + len <= p.count else { break }
                options[code, default: []] += p[(i + 2)..<(i + 2 + len)]
                i += 2 + len
            }
            guard let t = options[53]?.first, let type = MessageType(rawValue: t) else { return nil }
            let hlen = min(Int(p[2]), 16)
            guard hlen > 0 else { return nil }
            func ip(_ code: UInt8) -> IPv4? {
                guard let d = options[code], d.count == 4 else { return nil }
                return IPv4(value: d.reduce(0) { $0 << 8 | UInt32($1) })
            }
            func text(_ code: UInt8) -> String? {
                guard let d = options[code] else { return nil }
                let s = String(decoding: d.filter { $0 != 0 }, as: UTF8.self).trimmingWhitespace
                return s.isEmpty ? nil : s
            }
            raw = p
            self.type = type
            ciaddr = be32(12)
            giaddr = be32(24)
            mac = p[28..<(28 + hlen)].map { String(format: "%02x", $0) }.joined(separator: ":")
            requestedIP = ip(50)
            serverID = ip(54)
            hostname = text(12)
            vendor = text(60)
        }
    }
}
