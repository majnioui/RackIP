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

if failures == 0 { print("All checks passed.") } else { print("\(failures) check(s) failed."); exit(1) }
