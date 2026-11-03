// Plain executable instead of XCTest so it runs with only the Command Line
// Tools installed:  swift run core-checks
import Foundation
import RackIPCore

var failures = 0
func check(_ condition: Bool, _ message: String, line: Int = #line) {
    if !condition { failures += 1; print("FAIL line \(line): \(message)") }
}

func ok(_ a: String, _ m: String = "", _ g: String = "") -> ManualConfig? {
    if case .success(let c) = ManualConfig.parse(address: a, mask: m, gateway: g) { return c }
    return nil
}
func err(_ a: String, _ m: String = "", _ g: String = "") -> ConfigError? {
    if case .failure(let e) = ManualConfig.parse(address: a, mask: m, gateway: g) { return e }
    return nil
}

// IPv4 parsing
check(IPv4("10.0.0.5")?.description == "10.0.0.5", "round trip")
check(IPv4("255.255.255.255")?.value == .max, "max")
for bad in ["", "1.2.3", "1.2.3.4.5", "256.1.1.1", "1..2.3", "a.b.c.d", " 1.2.3.4", "1.2.3.-4", "0001.2.3.4", "+1.2.3.4"] {
    check(IPv4(bad) == nil, "should reject \"\(bad)\"")
}
check(IPv4.mask(prefix: 24)?.description == "255.255.255.0", "/24")
check(IPv4.mask(prefix: 0)?.value == 0, "/0")
check(IPv4.mask(prefix: 32)?.value == .max, "/32")
check(IPv4("255.255.0.255")?.prefixLength == nil, "non-contiguous mask")
check(IPv4("255.255.252.0")?.prefixLength == 22, "/22")

// Manual config
check(ok("192.168.1.10")?.mask.description == "255.255.255.0", "default mask")
check(ok(" 192.168.1.10 ")?.address.description == "192.168.1.10", "trims")
check(ok("10.1.2.3/16")?.mask.description == "255.255.0.0", "CIDR on address")
check(ok("10.1.2.3", "/22")?.mask.description == "255.255.252.0", "mask as /prefix")
check(ok("10.1.2.3", "22")?.mask.description == "255.255.252.0", "mask as prefix")
check(ok("10.1.2.3/16", "255.255.0.0") != nil, "matching CIDR and mask")
check(ok("10.0.0.5", "", "10.0.0.1")?.gateway?.description == "10.0.0.1", "gateway")
check(ok("10.0.0.5", "", "10.0.1.1")?.warnings.count == 1, "gateway outside subnet warns")
check(ok("10.0.0.5", "", "10.0.0.1")?.warnings.isEmpty == true, "gateway inside subnet")
check(ok("169.254.1.1", "255.255.0.0") != nil, "link-local allowed")
check(ok("10.0.0.0", "255.255.255.254") != nil, "/31 has no network address")

check(err("") == .missingAddress, "empty")
check(err("10.0.0") == .invalidAddress("10.0.0"), "bad address")
check(err("10.0.0.5", "255.0.255.0") == .invalidMask("255.0.255.0"), "bad mask")
check(err("10.0.0.5/33") == .invalidMask("33"), "bad prefix")
check(err("10.0.0.5", "", "x") == .invalidGateway("x"), "bad gateway")
check(err("10.0.0.5", "", "10.0.0.5") == .gatewayIsAddress, "gateway == ip")
check(err("10.0.0.0") != nil, "network address")
check(err("10.0.0.255") != nil, "broadcast address")
check(err("127.0.0.1", "255.0.0.0") != nil, "loopback")
check(err("224.0.0.1") != nil, "multicast")
if case .conflictingMask? = err("10.0.0.5/16", "255.255.255.0") {} else { check(false, "conflicting mask") }

// networksetup parsing
let order = """
An asterisk (*) denotes that a network service is disabled.
(1) USB 10/100/1000 LAN
(Hardware Port: USB 10/100/1000 LAN, Device: en5)

(2) Ethernet Mgmt
(Hardware Port: SZNX LAN 100M, Device: en11)

(*) Thunderbolt Bridge
(Hardware Port: Thunderbolt Bridge, Device: bridge0)

(4) Wi-Fi
(Hardware Port: Wi-Fi, Device: en0)

(5) VPN (Office)
(Hardware Port: com.fortinet.forticlient.macos.vpn, Device: )
"""
let services = NetworkSetupParser.services(fromServiceOrder: order)
check(services.map(\.name) == ["USB 10/100/1000 LAN", "Ethernet Mgmt", "Thunderbolt Bridge", "Wi-Fi", "VPN (Office)"], "service names: \(services.map(\.name))")
check(services.first?.device == "en5", "device")
check(services[1].hardwarePort == "SZNX LAN 100M", "hardware port")
check(services[2].enabled == false, "disabled")
check(services[4].device == "", "empty device")
check(services.map(\.isWired) == [true, true, false, false, false], "wired: \(services.map(\.isWired))")

let manual = NetworkSetupParser.info(fromGetInfo: """
Manual Configuration
IP address: 192.168.0.125
Subnet mask: 255.255.255.0
Router: (null)
IPv6: Automatic
Ethernet Address: (null)
""")
check(manual.mode == .manual && manual.address?.description == "192.168.0.125" && manual.router == nil, "manual info")
let dhcp = NetworkSetupParser.info(fromGetInfo: """
DHCP Configuration
IP address: 192.168.1.86
Subnet mask: 255.255.255.0
Router: 192.168.1.1
Client ID:
""")
check(dhcp.mode == .dhcp && dhcp.router?.description == "192.168.1.1", "dhcp info")


// DHCP config
func dhcp(_ server: String = "", _ mask: String = "", _ start: String = "", _ end: String = "", _ lease: String = "") -> Result<DHCPConfig, DHCPConfigError> {
    DHCPConfig.parse(server: server, mask: mask, rangeStart: start, rangeEnd: end, leaseMinutes: lease)
}
if case .success(let c) = dhcp() {
    check(c.server.description == "192.168.100.1" && c.rangeStart.description == "192.168.100.100"
          && c.rangeEnd.description == "192.168.100.200" && c.leaseSeconds == 3600, "dhcp defaults")
} else { check(false, "dhcp defaults parse") }
if case .success(let c) = dhcp("10.0.0.1/28") {
    check(c.rangeStart.description == "10.0.0.1" && c.rangeEnd.description == "10.0.0.14", "small subnet range")
} else { check(false, "small subnet parse") }
for (args, what) in [(("10.0.0.1", "", "10.0.1.5", "", ""), "range outside subnet"),
                     (("10.0.0.1", "", "10.0.0.50", "10.0.0.20", ""), "start after end"),
                     (("10.0.0.1", "", "", "", "0"), "lease zero"),
                     (("10.0.0.1", "/31", "", "", ""), "/31"),
                     (("10.0.0.1", "", "10.0.0.255", "", ""), "broadcast in range")] {
    if case .success = dhcp(args.0, args.1, args.2, args.3, args.4) { check(false, "should reject \(what)") }
}

// DHCP engine
func packet(_ type: UInt8, mac: UInt8, ciaddr: [UInt8] = [0, 0, 0, 0], options: [UInt8] = []) -> [UInt8] {
    var p = [UInt8](repeating: 0, count: 236)
    p[0] = 1; p[1] = 1; p[2] = 6
    p.replaceSubrange(4..<8, with: [0xde, 0xad, 0xbe, mac])
    p.replaceSubrange(12..<16, with: ciaddr)
    p.replaceSubrange(28..<34, with: [0x00, 0x11, 0x22, 0x33, 0x44, mac])
    return p + [99, 130, 83, 99, 53, 1, type] + options + [255]
}
func yiaddr(_ r: [UInt8]) -> String { r[16..<20].map(String.init).joined(separator: ".") }
func msgType(_ r: [UInt8]) -> UInt8? {
    var i = 240
    while i + 1 < r.count, r[i] != 255 { if r[i] == 53 { return r[i + 2] }; i += 2 + Int(r[i + 1]) }
    return nil
}
if case .success(let config) = dhcp("192.168.100.1", "", "192.168.100.100", "192.168.100.101") {
    var engine = DHCPEngine(config: config)
    let now = Date()
    let offer = engine.handle(packet(1, mac: 1, options: [12, 5] + Array("idrac".utf8)), now: now)
    check(offer.map { yiaddr($0.reply) } == "192.168.100.100" && offer.map { msgType($0.reply) } == 2, "offer first address")
    check(offer?.to == DHCPEngine.broadcast && offer?.reply[0] == 2 && offer?.reply[4..<8].elementsEqual([0xde, 0xad, 0xbe, 1]) == true, "offer header")
    check(engine.activeLeases(now: now).isEmpty, "offer is not a lease yet")

    let ack = engine.handle(packet(3, mac: 1, options: [50, 4, 192, 168, 100, 100, 54, 4, 192, 168, 100, 1]), now: now)
    check(ack.map { msgType($0.reply) } == 5 && ack.map { yiaddr($0.reply) } == "192.168.100.100", "ack")
    let active = engine.activeLeases(now: now)
    check(active.count == 1 && active.first?.mac == "00:11:22:33:44:01" && active.first?.hostname == "idrac", "lease recorded: \(active)")

    let second = engine.handle(packet(1, mac: 2), now: now)
    check(second.map { yiaddr($0.reply) } == "192.168.100.101", "second device gets the next address")
    _ = engine.handle(packet(3, mac: 2, options: [50, 4, 192, 168, 100, 101, 54, 4, 192, 168, 100, 1]), now: now)
    check(engine.handle(packet(1, mac: 3), now: now) == nil, "pool exhausted")

    let again = engine.handle(packet(1, mac: 1), now: now.addingTimeInterval(30))
    check(again.map { yiaddr($0.reply) } == "192.168.100.100", "returning device keeps its address")

    let wrong = engine.handle(packet(3, mac: 4, options: [50, 4, 10, 0, 0, 9]), now: now)
    check(wrong.map { msgType($0.reply) } == 6 && wrong?.to == DHCPEngine.broadcast, "nak for another network")

    let renew = engine.handle(packet(3, mac: 1, ciaddr: [192, 168, 100, 100]), now: now)
    check(renew.map { msgType($0.reply) } == 5 && renew?.to.description == "192.168.100.100", "renew is unicast")

    _ = engine.handle(packet(7, mac: 2, ciaddr: [192, 168, 100, 101]), now: now)
    check(engine.activeLeases(now: now).count == 1, "release frees the lease")

    check(engine.handle(packet(3, mac: 5, options: [50, 4, 10, 1, 1, 9, 54, 4, 10, 1, 1, 1]), now: now) == nil, "ignores another server's request")
    check(engine.foreignServer?.description == "10.1.1.1", "notes the other server")
    check(engine.handle(packet(1, mac: 6), now: now) == nil, "goes quiet after seeing another server")
} else { check(false, "engine config") }
var junkEngine = DHCPEngine(config: DHCPConfig(server: IPv4("10.0.0.1")!, mask: IPv4("255.255.255.0")!, rangeStart: IPv4("10.0.0.2")!, rangeEnd: IPv4("10.0.0.3")!, leaseSeconds: 60))
check(junkEngine.handle([1, 2, 3], now: Date()) == nil, "ignores junk")

if failures == 0 { print("All checks passed.") } else { print("\(failures) check(s) failed."); exit(1) }
