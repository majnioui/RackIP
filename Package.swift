// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "RackIP",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "rackip", targets: ["RackIP"]),
        .executable(name: "core-checks", targets: ["CoreChecks"]),
    ],
    targets: [
        // Address validation and `networksetup` output parsing. No AppKit, so
        // it is checked by core-checks and can be shared with a future port.
        .target(name: "RackIPCore"),
        .executableTarget(name: "RackIP", dependencies: ["RackIPCore"]),
        .executableTarget(name: "CoreChecks", dependencies: ["RackIPCore"]),
    ]
)
