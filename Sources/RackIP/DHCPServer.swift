import Darwin
import Foundation
import RackIPCore

/// Runs a `DHCPEngine` on UDP port 67 of one interface.
///
/// The socket is tied to the device with IP_BOUND_IF, so it neither hears nor
/// answers anything on Wi-Fi or other ports. macOS lets ordinary users bind
/// port 67 on the wildcard address, so no administrator rights are needed.
final class DHCPServer: @unchecked Sendable {
    struct SocketError: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    /// Called on the main queue whenever leases change or another server shows up.
    var onChange: (@MainActor ([DHCPLease], IPv4?) -> Void)?

    private let fd: Int32
    private let lock = NSLock()
    private var engine: DHCPEngine
    private var stopped = false

    init(device: String, config: DHCPConfig) throws {
        let index = if_nametoindex(device)
        guard index != 0 else { throw SocketError(message: "Could not find the network device \(device).") }
        let fd = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)
        guard fd >= 0 else { throw SocketError(message: "Could not open a network socket: \(String(cString: strerror(errno))).") }

        var yes: Int32 = 1
        var boundIndex = index
        var timeout = timeval(tv_sec: 0, tv_usec: 500_000)
        let ok = setsockopt(fd, SOL_SOCKET, SO_BROADCAST, &yes, socklen_t(MemoryLayout<Int32>.size)) == 0
            && setsockopt(fd, IPPROTO_IP, IP_BOUND_IF, &boundIndex, socklen_t(MemoryLayout<UInt32>.size)) == 0
            && setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size)) == 0
        guard ok else {
            let reason = String(cString: strerror(errno))
            close(fd)
            throw SocketError(message: "Could not set up the DHCP socket: \(reason).")
        }

        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = in_port_t(67).bigEndian
        addr.sin_addr = in_addr(s_addr: INADDR_ANY)
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard bound == 0 else {
            let code = errno
            close(fd)
            if code == EADDRINUSE {
                throw SocketError(message: "Another DHCP server is already running on this Mac (port 67 is in use). Turn off Internet Sharing and try again.")
            }
            throw SocketError(message: "Could not listen on port 67: \(String(cString: strerror(code))).")
        }

        self.fd = fd
        engine = DHCPEngine(config: config)
    }

    func start() {
        let thread = Thread { [self] in loop() }
        thread.name = "RackIP DHCP server"
        thread.start()
    }

    func stop() {
        lock.lock()
        stopped = true
        lock.unlock()
    }

    private var isStopped: Bool {
        lock.lock()
        defer { lock.unlock() }
        return stopped
    }

    private func loop() {
        defer { close(fd) }
        var buffer = [UInt8](repeating: 0, count: 1500)
        var lastReported: ([DHCPLease], IPv4?) = ([], nil)
        while !isStopped {
            let n = recv(fd, &buffer, buffer.count, 0)
            let now = Date()
            if n > 0 {
                let packet = Array(buffer[0..<n])
                if let (reply, to) = engine.handle(packet, now: now) {
                    send(reply, to: to)
                }
            }
            // Report after every packet, and on the timeout tick so expired leases disappear.
            let state = (engine.activeLeases(now: now), engine.foreignServer)
            if state.0 != lastReported.0 || state.1 != lastReported.1 {
                lastReported = state
                if let onChange {
                    DispatchQueue.main.async { MainActor.assumeIsolated { onChange(state.0, state.1) } }
                }
            }
        }
    }

    private func send(_ reply: [UInt8], to: IPv4) {
        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = in_port_t(68).bigEndian
        addr.sin_addr = in_addr(s_addr: to.value.bigEndian)
        _ = reply.withUnsafeBytes { bytes in
            withUnsafePointer(to: &addr) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    sendto(fd, bytes.baseAddress, bytes.count, 0, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                }
            }
        }
    }
}
