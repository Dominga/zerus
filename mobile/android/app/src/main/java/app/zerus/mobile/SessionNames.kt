package app.zerus.mobile

/** New tags only: existing session selectors keep their original spelling. */
object SessionNames {
    const val unsafeReason = "Names cannot contain shell syntax or invisible formatting characters."
    fun hasUnsafeSyntax(tag: String): Boolean = tag.codePoints().anyMatch { point ->
        point < 128 && "`$;&|<>(){}[]*?!#~\\'\",".contains(point.toChar()) || when (point) {
            // Unicode 17.0 General_Category=Cf, independent of the phone's Unicode version.
            in 0x2018..0x201e,
            0xad,
            in 0x600..0x605,
            0x61c,
            0x6dd,
            0x70f,
            in 0x890..0x891,
            0x8e2,
            0x180e,
            in 0x200b..0x200f,
            in 0x202a..0x202e,
            in 0x2060..0x2064,
            in 0x2066..0x206f,
            0xfeff,
            in 0xfff9..0xfffb,
            0x110bd,
            0x110cd,
            in 0x13430..0x1343f,
            in 0x1bca0..0x1bca3,
            in 0x1d173..0x1d17a,
            0xe0001,
            in 0xe0020..0xe007f -> true
            else -> false
        }
    }
}
