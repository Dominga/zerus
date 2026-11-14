import Foundation

/// New tags only. Existing session selectors keep their original spelling.
public enum SessionNames {
    public static let unsafeReason = "Names cannot contain shell syntax or invisible formatting characters."
    public static func hasUnsafeSyntax(_ tag: String) -> Bool {
        tag.unicodeScalars.contains { scalar in
            if "`$;&|<>(){}[]*?!#~\\'\",".unicodeScalars.contains(scalar) { return true }
            // Unicode 17.0 General_Category=Cf; same contract as the native CLI.
            switch scalar.value {
            case 0x2018...0x201e,
                 0xad,
                 0x600...0x605,
                 0x61c,
                 0x6dd,
                 0x70f,
                 0x890...0x891,
                 0x8e2,
                 0x180e,
                 0x200b...0x200f,
                 0x202a...0x202e,
                 0x2060...0x2064,
                 0x2066...0x206f,
                 0xfeff,
                 0xfff9...0xfffb,
                 0x110bd,
                 0x110cd,
                 0x13430...0x1343f,
                 0x1bca0...0x1bca3,
                 0x1d173...0x1d17a,
                 0xe0001,
                 0xe0020...0xe007f: return true
            default: return false
            }
        }
    }
}
