import Foundation
import RackIPCore

/// Talks to macOS through `/usr/sbin/networksetup` — the same backend as
/// System Settings › Network, so changes show up there too.
enum NetworkSetup {
    struct CommandError: LocalizedError {
        let message: String
        var errorDescription: String? { message }
    }

    private static let tool = "/usr/sbin/networksetup"

    static func services() throws -> [NetworkService] {
        let out = try run(tool, ["-listnetworkserviceorder"])
        return NetworkSetupParser.services(fromServiceOrder: out.output)
    }

    static func info(service: String) throws -> ServiceInfo {
        NetworkSetupParser.info(fromGetInfo: try run(tool, ["-getinfo", service]).output)
    }

    /// Whether a cable is plugged in and the link is up.
    static func isLinkActive(device: String) -> Bool {
        guard !device.isEmpty, let out = try? run("/sbin/ifconfig", [device]) else { return false }
        return out.output.contains("status: active")
    }

    static func setManual(service: String, config: ManualConfig) throws {
        var args = ["-setmanual", service, config.address.description, config.mask.description]
        if let gw = config.gateway { args.append(gw.description) }
        try change(args)
    }

    static func setDHCP(service: String) throws {
        try change(["-setdhcp", service])
    }

    /// Admin accounts can change network settings directly. Standard accounts
    /// get the system password prompt instead.
    private static func change(_ args: [String]) throws {
        let result = try run(tool, args)
        if let failure = failureMessage(result) {
            guard needsPrivileges(failure) else { throw CommandError(message: failure) }
            let elevated = try runElevated(tool, args)
            if let failure = failureMessage(elevated) { throw CommandError(message: failure) }
        }
    }

    /// networksetup often exits 0 even on failure, so read its output too.
    private static func failureMessage(_ r: (status: Int32, output: String)) -> String? {
        let text = r.output.trimmingCharacters(in: .whitespacesAndNewlines)
        let lower = text.lowercased()
        if r.status != 0 || lower.contains("error") || lower.contains("requires admin") || lower.contains("must run") {
            return text.isEmpty ? "networksetup exited with status \(r.status)." : text.replacingOccurrences(of: "** ", with: "")
        }
        return nil
    }

    private static func needsPrivileges(_ message: String) -> Bool {
        let m = message.lowercased()
        return ["admin", "root", "privilege", "authoriz", "permission", "not permitted"].contains(where: m.contains)
    }

    @discardableResult
    static func run(_ path: String, _ args: [String]) throws -> (status: Int32, output: String) {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = args
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe
        try process.run()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }

    /// Runs a command as root via the standard macOS authentication dialog.
    private static func runElevated(_ path: String, _ args: [String]) throws -> (status: Int32, output: String) {
        let shellQuoted = ([path] + args).map { "'" + $0.replacingOccurrences(of: "'", with: "'\\''") + "'" }.joined(separator: " ")
        let appleScriptString = shellQuoted
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
        let script = "do shell script \"\(appleScriptString) 2>&1\" with administrator privileges"
        let result = try run("/usr/bin/osascript", ["-e", script])
        if result.output.contains("User canceled") || result.output.contains("(-128)") {
            throw CommandError(message: "Cancelled — administrator rights are needed to change network settings.")
        }
        return result
    }
}
