/// An IPv4 address held as a 32-bit number, so masks and subnets are plain
/// bit arithmetic.
public struct IPv4: Equatable, Hashable, CustomStringConvertible {
    public let value: UInt32

    public init(value: UInt32) { self.value = value }

    /// Parses dotted-quad notation ("10.0.0.5"). Rejects anything else:
    /// missing octets, values over 255, signs, spaces, hex.
    public init?(_ text: String) {
        let parts = text.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else { return nil }
        var result: UInt32 = 0
        for part in parts {
            guard !part.isEmpty, part.count <= 3,
                  part.allSatisfy({ $0.isASCII && $0.isNumber }),
                  let octet = UInt32(part), octet <= 255 else { return nil }
            result = result << 8 | octet
        }
        value = result
    }

    public var description: String {
        [24, 16, 8, 0].map { String((value >> UInt32($0)) & 0xFF) }.joined(separator: ".")
    }

    /// The mask for a prefix length, e.g. 24 → 255.255.255.0.
    public static func mask(prefix: Int) -> IPv4? {
        guard (0...32).contains(prefix) else { return nil }
        return IPv4(value: prefix == 0 ? 0 : UInt32.max << UInt32(32 - prefix))
    }

    /// The prefix length if this is a valid netmask (contiguous ones), else nil.
    public var prefixLength: Int? {
        let ones = value.nonzeroBitCount
        return IPv4.mask(prefix: ones) == self ? ones : nil
    }

    public func network(mask: IPv4) -> UInt32 { value & mask.value }
}
