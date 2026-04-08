use std::fmt;

/// Byte/word-swap configuration for register conversions.
#[derive(Debug, Clone, Copy, Default)]
pub struct WordSwap {
    pub ints: bool,
    pub floats: bool,
    /// Word-swap all multi-register types (overrides ints/floats for convenience).
    pub words: bool,
    /// Byte-swap: reverse bytes within each u16 register (0xABCD → 0xCDAB).
    /// Applied to all register types (1, 2, and 4-register values).
    pub bytes: bool,
}

/// Numeric interpretation format for register values.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NumFormat {
    #[default]
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Float16,
    Float32,
    Float64,
    Bin16,
    Ascii,
}

impl std::str::FromStr for NumFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "u16" => Ok(Self::Uint16),
            "i16" => Ok(Self::Int16),
            "u32" => Ok(Self::Uint32),
            "i32" => Ok(Self::Int32),
            "u64" => Ok(Self::Uint64),
            "i64" => Ok(Self::Int64),
            "f32" => Ok(Self::Float32),
            "f64" => Ok(Self::Float64),
            "b16" => Ok(Self::Bin16),
            "ascii" => Ok(Self::Ascii),
            _ => Err(format!(
                "unknown format '{}'; expected one of: u16, i16, u32, i32, u64, i64, f32, f64, b16, ascii",
                s
            )),
        }
    }
}

impl NumFormat {
    /// All variants in display order.
    pub const ALL: &'static [NumFormat] = &[
        Self::Int16,
        Self::Uint16,
        Self::Int32,
        Self::Uint32,
        Self::Int64,
        Self::Uint64,
        Self::Float16,
        Self::Float32,
        Self::Float64,
        Self::Bin16,
        Self::Ascii,
    ];

    /// How many consecutive u16 registers this format consumes.
    pub fn width(self) -> usize {
        match self {
            Self::Int16 | Self::Uint16 | Self::Float16 | Self::Bin16 | Self::Ascii => 1,
            Self::Int32 | Self::Uint32 | Self::Float32 => 2,
            Self::Int64 | Self::Uint64 | Self::Float64 => 4,
        }
    }

    /// Short label for the column header.
    pub fn column_header(self) -> &'static str {
        match self {
            Self::Int16 => "Int16",
            Self::Uint16 => "Uint16",
            Self::Int32 => "Int32",
            Self::Uint32 => "Uint32",
            Self::Int64 => "Int64",
            Self::Uint64 => "Uint64",
            Self::Float16 => "Float16",
            Self::Float32 => "Float32",
            Self::Float64 => "Float64",
            Self::Bin16 => "Bin16",
            Self::Ascii => "Ascii",
        }
    }

    /// Whether this format should have its words (register order) swapped.
    /// Only applies to multi-register types (width >= 2).
    pub fn should_swap(self, ws: &WordSwap) -> bool {
        if self.width() < 2 {
            return false;
        }
        if ws.words {
            return true;
        }
        match self {
            Self::Int32 | Self::Uint32 | Self::Int64 | Self::Uint64 => ws.ints,
            Self::Float32 | Self::Float64 => ws.floats,
            _ => false,
        }
    }

    /// Convert a slice of registers to a display string.
    /// If byte-swap is active, bytes within each register are swapped first.
    /// If word-swap applies, word order is then reversed before conversion.
    pub fn format_value(self, regs: &[u16], ws: &WordSwap) -> String {
        if regs.len() < self.width() {
            return "?".to_string();
        }
        // 1. Byte-swap each register if configured
        let mut r: Vec<u16> = if ws.bytes {
            regs[..self.width()]
                .iter()
                .map(|&v| v.swap_bytes())
                .collect()
        } else {
            regs[..self.width()].to_vec()
        };
        // 2. Word-swap (swap 32-bit halves) if configured for this format type
        //    2 regs: [A, B] → [B, A]
        //    4 regs: [A, B, C, D] → [C, D, A, B]
        if self.should_swap(ws) {
            let half = r.len() / 2;
            r.rotate_left(half);
        }
        match self {
            Self::Int16 => format!("{}", r[0] as i16),
            Self::Uint16 => format!("{}", r[0]),
            Self::Bin16 => format!("{:016b}", r[0]),
            Self::Float16 => format!("{:.4}", f16_to_f32(r[0])),
            Self::Ascii => {
                let hi = (r[0] >> 8) as u8;
                let lo = (r[0] & 0xFF) as u8;
                let to_char = |b: u8| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' };
                format!("{}{}", to_char(hi), to_char(lo))
            }

            Self::Int32 => {
                let v = combine_u32(r[0], r[1]);
                format!("{}", v as i32)
            }
            Self::Uint32 => {
                let v = combine_u32(r[0], r[1]);
                format!("{}", v)
            }
            Self::Float32 => {
                let v = combine_u32(r[0], r[1]);
                format!("{:.6}", f32::from_bits(v))
            }

            Self::Int64 => {
                let v = combine_u64(r[0], r[1], r[2], r[3]);
                format!("{}", v as i64)
            }
            Self::Uint64 => {
                let v = combine_u64(r[0], r[1], r[2], r[3]);
                format!("{}", v)
            }
            Self::Float64 => {
                let v = combine_u64(r[0], r[1], r[2], r[3]);
                format!("{:.10}", f64::from_bits(v))
            }
        }
    }

    /// Parse a user-provided string into register values.
    /// If word-swap is active for this format, the output registers are reversed
    /// so they can be written directly to the device in swapped order.
    pub fn parse_value(self, input: &str, ws: &WordSwap) -> Result<Vec<u16>, String> {
        let trimmed = input.trim();
        let mut regs = match self {
            Self::Int16 => {
                let v = parse_int::<i16>(trimmed).map_err(|e| format!("Int16: {e}"))?;
                vec![v as u16]
            }
            Self::Uint16 => {
                let v = parse_uint::<u16>(trimmed).map_err(|e| format!("Uint16: {e}"))?;
                vec![v]
            }
            Self::Bin16 => {
                let v = parse_uint::<u16>(trimmed).map_err(|e| format!("Bin16: {e}"))?;
                vec![v]
            }
            Self::Float16 => {
                let f: f32 = trimmed.parse().map_err(|e| format!("Float16: {e}"))?;
                vec![f32_to_f16(f)]
            }
            Self::Ascii => {
                let bytes: Vec<u8> = trimmed.bytes().collect();
                if bytes.len() != 2 {
                    return Err("Ascii: expected exactly 2 characters".to_string());
                }
                vec![((bytes[0] as u16) << 8) | (bytes[1] as u16)]
            }
            Self::Int32 => {
                let v = parse_int::<i32>(trimmed).map_err(|e| format!("Int32: {e}"))?;
                split_u32(v as u32)
            }
            Self::Uint32 => {
                let v = parse_uint::<u32>(trimmed).map_err(|e| format!("Uint32: {e}"))?;
                split_u32(v)
            }
            Self::Float32 => {
                let f: f32 = trimmed.parse().map_err(|e| format!("Float32: {e}"))?;
                split_u32(f.to_bits())
            }
            Self::Int64 => {
                let v = parse_int::<i64>(trimmed).map_err(|e| format!("Int64: {e}"))?;
                split_u64(v as u64)
            }
            Self::Uint64 => {
                let v = parse_uint::<u64>(trimmed).map_err(|e| format!("Uint64: {e}"))?;
                split_u64(v)
            }
            Self::Float64 => {
                let f: f64 = trimmed.parse().map_err(|e| format!("Float64: {e}"))?;
                split_u64(f.to_bits())
            }
        };
        // Swap 32-bit halves for write if swap is active
        //    2 regs: [A, B] → [B, A]
        //    4 regs: [A, B, C, D] → [C, D, A, B]
        if self.should_swap(ws) {
            let half = regs.len() / 2;
            regs.rotate_left(half);
        }
        // Byte-swap each register for write if configured
        if ws.bytes {
            for reg in &mut regs {
                *reg = reg.swap_bytes();
            }
        }
        Ok(regs)
    }
}

impl fmt::Display for NumFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (name, desc) = match self {
            Self::Int16 => ("Int16", "Signed 16-bit integer (1 reg)"),
            Self::Uint16 => ("Uint16", "Unsigned 16-bit integer (1 reg)"),
            Self::Int32 => ("Int32", "Signed 32-bit integer (2 regs)"),
            Self::Uint32 => ("Uint32", "Unsigned 32-bit integer (2 regs)"),
            Self::Int64 => ("Int64", "Signed 64-bit integer (4 regs)"),
            Self::Uint64 => ("Uint64", "Unsigned 64-bit integer (4 regs)"),
            Self::Float16 => ("Float16", "IEEE 754 half-precision (1 reg)"),
            Self::Float32 => ("Float32", "IEEE 754 single-precision (2 regs)"),
            Self::Float64 => ("Float64", "IEEE 754 double-precision (4 regs)"),
            Self::Bin16 => ("Bin16", "Binary 16-bit (1 reg)"),
            Self::Ascii => ("Ascii", "ASCII string (2 chars/reg)"),
        };
        write!(f, "{:<10} {}", name, desc)
    }
}

// ---------------------------------------------------------------------------
// Parsing helpers — support dec, 0x hex, 0b binary
// ---------------------------------------------------------------------------

fn parse_int<T>(s: &str) -> Result<T, String>
where
    T: std::str::FromStr + TryFrom<i64>,
    <T as std::str::FromStr>::Err: fmt::Display,
    <T as TryFrom<i64>>::Error: fmt::Display,
{
    let val: i64 = if s.starts_with("0x") || s.starts_with("0X") {
        i64::from_str_radix(&s[2..], 16).map_err(|e| e.to_string())?
    } else if s.starts_with("0b") || s.starts_with("0B") {
        i64::from_str_radix(&s[2..], 2).map_err(|e| e.to_string())?
    } else if s.starts_with('-') {
        s.parse::<i64>().map_err(|e| e.to_string())?
    } else {
        s.parse::<i64>().map_err(|e| e.to_string())?
    };
    T::try_from(val).map_err(|e| format!("out of range: {e}"))
}

fn parse_uint<T>(s: &str) -> Result<T, String>
where
    T: std::str::FromStr + TryFrom<u64>,
    <T as std::str::FromStr>::Err: fmt::Display,
    <T as TryFrom<u64>>::Error: fmt::Display,
{
    let val: u64 = if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).map_err(|e| e.to_string())?
    } else if s.starts_with("0b") || s.starts_with("0B") {
        u64::from_str_radix(&s[2..], 2).map_err(|e| e.to_string())?
    } else {
        s.parse::<u64>().map_err(|e| e.to_string())?
    };
    T::try_from(val).map_err(|e| format!("out of range: {e}"))
}

// ---------------------------------------------------------------------------
// Splitting values into registers (big-endian / high-word-first)
// ---------------------------------------------------------------------------

fn split_u32(v: u32) -> Vec<u16> {
    vec![(v >> 16) as u16, v as u16]
}

fn split_u64(v: u64) -> Vec<u16> {
    vec![
        (v >> 48) as u16,
        (v >> 32) as u16,
        (v >> 16) as u16,
        v as u16,
    ]
}

// ---------------------------------------------------------------------------
// f32 → IEEE 754 half-precision (float16) conversion
// ---------------------------------------------------------------------------

pub(crate) fn f32_to_f16(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x007f_ffff;

    if exp == 255 {
        // Inf or NaN
        if frac == 0 {
            return sign | 0x7c00; // Inf
        } else {
            return sign | 0x7c00 | ((frac >> 13) as u16).max(1); // NaN
        }
    }

    let unbiased = exp - 127;
    if unbiased > 15 {
        // Overflow → Inf
        return sign | 0x7c00;
    }
    if unbiased < -24 {
        // Too small → zero
        return sign;
    }
    if unbiased < -14 {
        // Subnormal in f16
        let shift = -14 - unbiased;
        let f = (frac | 0x0080_0000) >> (13 + shift);
        return sign | f as u16;
    }
    let e = (unbiased + 15) as u16;
    sign | (e << 10) | ((frac >> 13) as u16)
}

// ---------------------------------------------------------------------------
// Combining registers (big-endian / high-word-first, standard Modbus order)
// ---------------------------------------------------------------------------

fn combine_u32(hi: u16, lo: u16) -> u32 {
    ((hi as u32) << 16) | (lo as u32)
}

fn combine_u64(w0: u16, w1: u16, w2: u16, w3: u16) -> u64 {
    ((w0 as u64) << 48) | ((w1 as u64) << 32) | ((w2 as u64) << 16) | (w3 as u64)
}

// ---------------------------------------------------------------------------
// IEEE 754 half-precision (float16) → f32 conversion
// ---------------------------------------------------------------------------

pub(crate) fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let frac = (bits & 0x3ff) as u32;

    if exp == 0 {
        if frac == 0 {
            // +/- zero
            f32::from_bits(sign << 31)
        } else {
            // Subnormal: convert to normalized f32
            let mut e = exp;
            let mut f = frac;
            while (f & 0x400) == 0 {
                f <<= 1;
                e += 1;
            }
            f &= 0x3ff;
            let new_exp = 127 - 15 - e + 1;
            f32::from_bits((sign << 31) | (new_exp << 23) | (f << 13))
        }
    } else if exp == 31 {
        if frac == 0 {
            // Infinity
            f32::from_bits((sign << 31) | (0xff << 23))
        } else {
            // NaN
            f32::from_bits((sign << 31) | (0xff << 23) | (frac << 13))
        }
    } else {
        // Normal: rebias exponent from 15-bias to 127-bias
        let new_exp = exp + 112;
        f32::from_bits((sign << 31) | (new_exp << 23) | (frac << 13))
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const NO_SWAP: WordSwap = WordSwap {
        ints: false,
        floats: false,
        words: false,
        bytes: false,
    };
    const BYTE_SWAP: WordSwap = WordSwap {
        ints: false,
        floats: false,
        words: false,
        bytes: true,
    };
    const WORD_SWAP_INTS: WordSwap = WordSwap {
        ints: true,
        floats: false,
        words: false,
        bytes: false,
    };
    const WORD_SWAP_FLOATS: WordSwap = WordSwap {
        ints: false,
        floats: true,
        words: false,
        bytes: false,
    };
    const WORD_SWAP_ALL: WordSwap = WordSwap {
        ints: true,
        floats: true,
        words: false,
        bytes: false,
    };
    const BYTE_AND_WORD_INTS: WordSwap = WordSwap {
        ints: true,
        floats: false,
        words: false,
        bytes: true,
    };
    const BYTE_AND_WORD_FLOATS: WordSwap = WordSwap {
        ints: false,
        floats: true,
        words: false,
        bytes: true,
    };
    const BYTE_AND_WORD_ALL: WordSwap = WordSwap {
        ints: true,
        floats: true,
        words: false,
        bytes: true,
    };
    const SWAP_WORDS: WordSwap = WordSwap {
        ints: false,
        floats: false,
        words: true,
        bytes: false,
    };
    const SWAP_WORDS_AND_BYTES: WordSwap = WordSwap {
        ints: false,
        floats: false,
        words: true,
        bytes: true,
    };

    // -----------------------------------------------------------------------
    // Helper: round-trip from wire registers through format then parse
    // -----------------------------------------------------------------------

    /// Verify that format_value → parse_value returns the original wire registers.
    fn assert_roundtrip(nf: NumFormat, regs: &[u16], ws: &WordSwap) {
        let display = nf.format_value(regs, ws);
        assert_ne!(display, "?", "format_value returned '?' for {:?}", nf);
        let parsed = nf.parse_value(&display, ws).unwrap_or_else(|e| {
            panic!("parse_value failed for {:?} input='{}': {}", nf, display, e)
        });
        assert_eq!(
            &parsed,
            &regs[..nf.width()],
            "round-trip failed for {:?} ws={:?}\n  wire_regs={:?}\n  display='{}'\n  parsed={:?}",
            nf,
            ws,
            &regs[..nf.width()],
            display,
            parsed
        );
    }

    /// Verify parse → format → parse produces the same wire registers.
    /// Useful for float types where arbitrary registers may not survive display.
    fn assert_value_roundtrip(nf: NumFormat, value: &str, ws: &WordSwap) {
        let wire = nf
            .parse_value(value, ws)
            .unwrap_or_else(|e| panic!("parse_value failed for {:?} input='{}': {}", nf, value, e));
        let display = nf.format_value(&wire, ws);
        let wire2 = nf.parse_value(&display, ws).unwrap_or_else(|e| {
            panic!("re-parse failed for {:?} display='{}': {}", nf, display, e)
        });
        assert_eq!(
            wire, wire2,
            "value roundtrip failed for {:?} ws={:?}\n  input='{}'\n  wire={:?}\n  display='{}'\n  wire2={:?}",
            nf, ws, value, wire, display, wire2
        );
    }

    // =======================================================================
    // 1. Baseline: no swap — verify format_value for all types
    // =======================================================================

    #[test]
    fn format_uint16_no_swap() {
        assert_eq!(NumFormat::Uint16.format_value(&[258], &NO_SWAP), "258");
        assert_eq!(NumFormat::Uint16.format_value(&[0], &NO_SWAP), "0");
        assert_eq!(NumFormat::Uint16.format_value(&[65535], &NO_SWAP), "65535");
    }

    #[test]
    fn format_int16_no_swap() {
        assert_eq!(NumFormat::Int16.format_value(&[0], &NO_SWAP), "0");
        assert_eq!(NumFormat::Int16.format_value(&[32767], &NO_SWAP), "32767");
        // 0xFFFF = -1 as i16
        assert_eq!(NumFormat::Int16.format_value(&[0xFFFF], &NO_SWAP), "-1");
        // 0x8000 = -32768
        assert_eq!(NumFormat::Int16.format_value(&[0x8000], &NO_SWAP), "-32768");
    }

    #[test]
    fn format_bin16_no_swap() {
        assert_eq!(
            NumFormat::Bin16.format_value(&[0b1010_0000_0000_0101], &NO_SWAP),
            "1010000000000101"
        );
        assert_eq!(
            NumFormat::Bin16.format_value(&[0], &NO_SWAP),
            "0000000000000000"
        );
    }

    #[test]
    fn format_uint32_no_swap() {
        // 0x00010002 = 65538, regs = [0x0001, 0x0002]
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0001, 0x0002], &NO_SWAP),
            "65538"
        );
    }

    #[test]
    fn format_int32_no_swap() {
        // -1 as i32 = 0xFFFFFFFF, regs = [0xFFFF, 0xFFFF]
        assert_eq!(
            NumFormat::Int32.format_value(&[0xFFFF, 0xFFFF], &NO_SWAP),
            "-1"
        );
    }

    #[test]
    fn format_float32_no_swap() {
        // 1.0f32 = 0x3F800000, regs = [0x3F80, 0x0000]
        assert_eq!(
            NumFormat::Float32.format_value(&[0x3F80, 0x0000], &NO_SWAP),
            "1.000000"
        );
    }

    #[test]
    fn format_uint64_no_swap() {
        // 1 as u64 = 0x0000000000000001, regs = [0, 0, 0, 1]
        assert_eq!(NumFormat::Uint64.format_value(&[0, 0, 0, 1], &NO_SWAP), "1");
    }

    #[test]
    fn format_int64_no_swap() {
        // -1 as i64 = all-ones
        assert_eq!(
            NumFormat::Int64.format_value(&[0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF], &NO_SWAP),
            "-1"
        );
    }

    #[test]
    fn format_float64_no_swap() {
        // 1.0f64 = 0x3FF0000000000000, regs = [0x3FF0, 0x0000, 0x0000, 0x0000]
        assert_eq!(
            NumFormat::Float64.format_value(&[0x3FF0, 0x0000, 0x0000, 0x0000], &NO_SWAP),
            "1.0000000000"
        );
    }

    #[test]
    fn format_float16_no_swap() {
        // f16 1.0 = 0x3C00
        let result = NumFormat::Float16.format_value(&[0x3C00], &NO_SWAP);
        assert_eq!(result, "1.0000");
    }

    // =======================================================================
    // 2. Byte-swap — verify byte swap effect on all register widths
    // =======================================================================

    #[test]
    fn byte_swap_uint16() {
        // 0x0102 → byte-swapped → 0x0201 = 513
        assert_eq!(NumFormat::Uint16.format_value(&[0x0102], &BYTE_SWAP), "513");
    }

    #[test]
    fn byte_swap_int16() {
        // 0x00FF → byte-swapped → 0xFF00 = -256 as i16
        assert_eq!(NumFormat::Int16.format_value(&[0x00FF], &BYTE_SWAP), "-256");
    }

    #[test]
    fn byte_swap_bin16() {
        // 0x0102 = 0000_0001_0000_0010 → byte-swapped → 0x0201 = 0000_0010_0000_0001
        assert_eq!(
            NumFormat::Bin16.format_value(&[0x0102], &BYTE_SWAP),
            "0000001000000001"
        );
    }

    #[test]
    fn byte_swap_float16() {
        // f16 1.0 = 0x3C00 → byte-swapped → 0x003C
        // 0x003C is a subnormal f16 — different from 1.0
        let no_swap = NumFormat::Float16.format_value(&[0x3C00], &NO_SWAP);
        let swapped = NumFormat::Float16.format_value(&[0x3C00], &BYTE_SWAP);
        assert_ne!(
            no_swap, swapped,
            "byte swap should change Float16 interpretation"
        );
        assert_eq!(no_swap, "1.0000");
    }

    #[test]
    fn byte_swap_uint32() {
        // regs [0x0102, 0x0304] → byte-swapped → [0x0201, 0x0403]
        // combine_u32(0x0201, 0x0403) = 0x02010403 = 33620995
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0102, 0x0304], &BYTE_SWAP),
            "33620995"
        );
        // Without swap: combine_u32(0x0102, 0x0304) = 0x01020304 = 16909060
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0102, 0x0304], &NO_SWAP),
            "16909060"
        );
    }

    #[test]
    fn byte_swap_int32() {
        // regs [0x0001, 0x0000] → no swap → 0x00010000 = 65536
        // byte-swapped → [0x0100, 0x0000] → 0x01000000 = 16777216
        assert_eq!(
            NumFormat::Int32.format_value(&[0x0001, 0x0000], &BYTE_SWAP),
            "16777216"
        );
    }

    #[test]
    fn byte_swap_float32() {
        // 1.0f32 = 0x3F800000 → regs = [0x3F80, 0x0000]
        // byte-swapped → [0x803F, 0x0000] → 0x803F0000 ≠ 1.0
        let no_swap = NumFormat::Float32.format_value(&[0x3F80, 0x0000], &NO_SWAP);
        let swapped = NumFormat::Float32.format_value(&[0x3F80, 0x0000], &BYTE_SWAP);
        assert_eq!(no_swap, "1.000000");
        assert_ne!(
            swapped, "1.000000",
            "byte swap should change f32 interpretation"
        );
    }

    #[test]
    fn byte_swap_uint64() {
        // regs [0x0001, 0x0000, 0x0000, 0x0000] → no swap → 1 << 48
        // byte-swapped → [0x0100, 0x0000, 0x0000, 0x0000] → 0x01000000_00000000
        assert_eq!(
            NumFormat::Uint64.format_value(&[0x0001, 0x0000, 0x0000, 0x0000], &NO_SWAP),
            "281474976710656" // 0x0001_0000_0000_0000
        );
        assert_eq!(
            NumFormat::Uint64.format_value(&[0x0001, 0x0000, 0x0000, 0x0000], &BYTE_SWAP),
            "72057594037927936" // 0x0100_0000_0000_0000
        );
    }

    #[test]
    fn byte_swap_int64() {
        let no_swap = NumFormat::Int64.format_value(&[0x0001, 0x0000, 0x0000, 0x0000], &NO_SWAP);
        let swapped = NumFormat::Int64.format_value(&[0x0001, 0x0000, 0x0000, 0x0000], &BYTE_SWAP);
        assert_ne!(
            no_swap, swapped,
            "byte swap should change i64 interpretation"
        );
    }

    #[test]
    fn byte_swap_float64() {
        let no_swap = NumFormat::Float64.format_value(&[0x3FF0, 0x0000, 0x0000, 0x0000], &NO_SWAP);
        let swapped =
            NumFormat::Float64.format_value(&[0x3FF0, 0x0000, 0x0000, 0x0000], &BYTE_SWAP);
        assert_eq!(no_swap, "1.0000000000");
        assert_ne!(swapped, "1.0000000000");
    }

    // =======================================================================
    // 3. Word-swap — verify it only applies to multi-register types
    // =======================================================================

    #[test]
    fn word_swap_does_not_affect_single_reg() {
        // Word swap should have no effect on 1-register formats
        assert_eq!(
            NumFormat::Uint16.format_value(&[0x0102], &WORD_SWAP_INTS),
            NumFormat::Uint16.format_value(&[0x0102], &NO_SWAP)
        );
        assert_eq!(
            NumFormat::Int16.format_value(&[0x0102], &WORD_SWAP_INTS),
            NumFormat::Int16.format_value(&[0x0102], &NO_SWAP)
        );
        assert_eq!(
            NumFormat::Bin16.format_value(&[0x0102], &WORD_SWAP_INTS),
            NumFormat::Bin16.format_value(&[0x0102], &NO_SWAP)
        );
        assert_eq!(
            NumFormat::Float16.format_value(&[0x3C00], &WORD_SWAP_FLOATS),
            NumFormat::Float16.format_value(&[0x3C00], &NO_SWAP)
        );
    }

    #[test]
    fn word_swap_uint32() {
        // [0x0001, 0x0002] → reversed → [0x0002, 0x0001] → 0x00020001 = 131073
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0001, 0x0002], &WORD_SWAP_INTS),
            "131073"
        );
        // Without swap: 0x00010002 = 65538
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0001, 0x0002], &NO_SWAP),
            "65538"
        );
    }

    #[test]
    fn word_swap_float32() {
        // 1.0 = 0x3F800000 → regs [0x3F80, 0x0000]
        // word swapped → [0x0000, 0x3F80] → 0x00003F80 → not 1.0
        let swapped = NumFormat::Float32.format_value(&[0x3F80, 0x0000], &WORD_SWAP_FLOATS);
        assert_ne!(swapped, "1.000000");
    }

    #[test]
    fn word_swap_ints_does_not_affect_floats() {
        // swap_ints should NOT swap float formats
        assert_eq!(
            NumFormat::Float32.format_value(&[0x3F80, 0x0000], &WORD_SWAP_INTS),
            NumFormat::Float32.format_value(&[0x3F80, 0x0000], &NO_SWAP)
        );
    }

    #[test]
    fn word_swap_floats_does_not_affect_ints() {
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0001, 0x0002], &WORD_SWAP_FLOATS),
            NumFormat::Uint32.format_value(&[0x0001, 0x0002], &NO_SWAP)
        );
    }

    // =======================================================================
    // 4. Combined byte + word swap
    // =======================================================================

    #[test]
    fn byte_and_word_swap_uint32() {
        // regs [0x0102, 0x0304]
        // Step 1 (byte swap): [0x0201, 0x0403]
        // Step 2 (word swap): [0x0403, 0x0201]
        // combine_u32(0x0403, 0x0201) = 0x04030201 = 67305985
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0102, 0x0304], &BYTE_AND_WORD_INTS),
            "67305985"
        );
    }

    #[test]
    fn byte_and_word_swap_uint64() {
        // regs [0x0102, 0x0304, 0x0506, 0x0708]
        // Step 1 (byte swap): [0x0201, 0x0403, 0x0605, 0x0807]
        // Step 2 (word swap — swap 32-bit halves): [0x0605, 0x0807, 0x0201, 0x0403]
        // combine = 0x0605080702010403
        assert_eq!(
            NumFormat::Uint64.format_value(&[0x0102, 0x0304, 0x0506, 0x0708], &BYTE_AND_WORD_INTS),
            "433761765302535171" // 0x0605080702010403
        );
    }

    #[test]
    fn byte_and_word_swap_float32() {
        // Verify byte+word swap produces a different result than no-swap
        let regs = [0x3F80, 0x0000];
        let v_none = NumFormat::Float32.format_value(&regs, &NO_SWAP);
        let v_both = NumFormat::Float32.format_value(&regs, &BYTE_AND_WORD_FLOATS);
        assert_eq!(v_none, "1.000000");
        assert_ne!(
            v_none, v_both,
            "byte+word swap should change f32 interpretation"
        );
    }

    // =======================================================================
    // 5. parse_value — byte swap produces correct wire registers
    // =======================================================================

    #[test]
    fn parse_uint16_byte_swap() {
        // User types "513" → u16 = 0x0201 → byte-swapped for wire → 0x0102
        let regs = NumFormat::Uint16.parse_value("513", &BYTE_SWAP).unwrap();
        assert_eq!(regs, vec![0x0102]);
    }

    #[test]
    fn parse_int16_byte_swap() {
        // User types "-256" → i16 = -256 → 0xFF00 → byte-swapped → 0x00FF
        let regs = NumFormat::Int16.parse_value("-256", &BYTE_SWAP).unwrap();
        assert_eq!(regs, vec![0x00FF]);
    }

    #[test]
    fn parse_uint32_byte_swap() {
        // User types "33620995" (= 0x02010403) → split → [0x0201, 0x0403]
        // byte-swapped for wire → [0x0102, 0x0304]
        let regs = NumFormat::Uint32
            .parse_value("33620995", &BYTE_SWAP)
            .unwrap();
        assert_eq!(regs, vec![0x0102, 0x0304]);
    }

    #[test]
    fn parse_uint32_word_swap() {
        // User types "131073" (= 0x00020001) → split → [0x0002, 0x0001]
        // word-swapped for wire → [0x0001, 0x0002]
        let regs = NumFormat::Uint32
            .parse_value("131073", &WORD_SWAP_INTS)
            .unwrap();
        assert_eq!(regs, vec![0x0001, 0x0002]);
    }

    #[test]
    fn parse_uint32_byte_and_word_swap() {
        // User types "67305985" (= 0x04030201) → split → [0x0403, 0x0201]
        // word-swapped → [0x0201, 0x0403]
        // byte-swapped → [0x0102, 0x0304]
        let regs = NumFormat::Uint32
            .parse_value("67305985", &BYTE_AND_WORD_INTS)
            .unwrap();
        assert_eq!(regs, vec![0x0102, 0x0304]);
    }

    #[test]
    fn parse_uint64_byte_and_word_swap() {
        // User types "433761765302535171" (= 0x0605080702010403)
        // split → [0x0605, 0x0807, 0x0201, 0x0403]
        // word-swapped (swap 32-bit halves) → [0x0201, 0x0403, 0x0605, 0x0807]
        // byte-swapped → [0x0102, 0x0304, 0x0506, 0x0708]
        let regs = NumFormat::Uint64
            .parse_value("433761765302535171", &BYTE_AND_WORD_INTS)
            .unwrap();
        assert_eq!(regs, vec![0x0102, 0x0304, 0x0506, 0x0708]);
    }

    // =======================================================================
    // 6. Round-trip tests — format then parse returns original wire registers
    // =======================================================================

    #[test]
    fn roundtrip_no_swap() {
        assert_roundtrip(NumFormat::Uint16, &[0x1234], &NO_SWAP);
        assert_roundtrip(NumFormat::Int16, &[0x8000], &NO_SWAP);
        // Bin16 display omits "0b" prefix so parse can't round-trip it;
        // tested separately via parse_binary_input tests.
        assert_roundtrip(NumFormat::Float16, &[0x3C00], &NO_SWAP); // 1.0
        assert_roundtrip(NumFormat::Uint32, &[0x0001, 0x0002], &NO_SWAP);
        assert_roundtrip(NumFormat::Int32, &[0xFFFF, 0xFFFF], &NO_SWAP);
        assert_roundtrip(NumFormat::Float32, &[0x3F80, 0x0000], &NO_SWAP);
        assert_roundtrip(NumFormat::Uint64, &[0, 0, 0, 1], &NO_SWAP);
        assert_roundtrip(
            NumFormat::Int64,
            &[0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF],
            &NO_SWAP,
        );
        assert_roundtrip(NumFormat::Float64, &[0x3FF0, 0, 0, 0], &NO_SWAP);
    }

    #[test]
    fn roundtrip_byte_swap_single_reg() {
        assert_roundtrip(NumFormat::Uint16, &[0x0102], &BYTE_SWAP);
        assert_roundtrip(NumFormat::Int16, &[0x00FF], &BYTE_SWAP);
        // Float16 byte-swap: use value-based roundtrip (display precision too limited
        // for arbitrary registers after byte-swap)
        assert_value_roundtrip(NumFormat::Float16, "1.0", &BYTE_SWAP);
    }

    #[test]
    fn roundtrip_byte_swap_dual_reg() {
        assert_roundtrip(NumFormat::Uint32, &[0x0102, 0x0304], &BYTE_SWAP);
        assert_roundtrip(NumFormat::Int32, &[0x0102, 0x0304], &BYTE_SWAP);
        assert_value_roundtrip(NumFormat::Float32, "42.0", &BYTE_SWAP);
    }

    #[test]
    fn roundtrip_byte_swap_quad_reg() {
        assert_roundtrip(
            NumFormat::Uint64,
            &[0x0102, 0x0304, 0x0506, 0x0708],
            &BYTE_SWAP,
        );
        assert_roundtrip(
            NumFormat::Int64,
            &[0x0102, 0x0304, 0x0506, 0x0708],
            &BYTE_SWAP,
        );
        assert_value_roundtrip(NumFormat::Float64, "42.0", &BYTE_SWAP);
    }

    #[test]
    fn roundtrip_word_swap() {
        assert_roundtrip(NumFormat::Uint32, &[0x0001, 0x0002], &WORD_SWAP_INTS);
        assert_roundtrip(NumFormat::Int32, &[0xFFFF, 0x0001], &WORD_SWAP_INTS);
        assert_value_roundtrip(NumFormat::Float32, "42.0", &WORD_SWAP_FLOATS);
        assert_roundtrip(NumFormat::Uint64, &[0, 0, 0, 1], &WORD_SWAP_INTS);
        assert_roundtrip(
            NumFormat::Int64,
            &[0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF],
            &WORD_SWAP_INTS,
        );
        assert_value_roundtrip(NumFormat::Float64, "42.0", &WORD_SWAP_FLOATS);
    }

    #[test]
    fn roundtrip_byte_and_word_swap() {
        assert_roundtrip(NumFormat::Uint32, &[0x0102, 0x0304], &BYTE_AND_WORD_INTS);
        assert_roundtrip(NumFormat::Int32, &[0x0102, 0x0304], &BYTE_AND_WORD_INTS);
        assert_value_roundtrip(NumFormat::Float32, "42.0", &BYTE_AND_WORD_FLOATS);
        assert_roundtrip(
            NumFormat::Uint64,
            &[0x0102, 0x0304, 0x0506, 0x0708],
            &BYTE_AND_WORD_INTS,
        );
        assert_roundtrip(
            NumFormat::Int64,
            &[0x0102, 0x0304, 0x0506, 0x0708],
            &BYTE_AND_WORD_INTS,
        );
        assert_value_roundtrip(NumFormat::Float64, "42.0", &BYTE_AND_WORD_FLOATS);
    }

    /// Exhaustive round-trip across integer/uint formats and ALL swap configurations.
    /// Float and Bin16 formats are tested separately because they have precision
    /// or formatting constraints that prevent universal round-trip.
    #[test]
    fn roundtrip_integer_formats_all_swaps() {
        let swap_configs = [
            NO_SWAP,
            BYTE_SWAP,
            WORD_SWAP_INTS,
            WORD_SWAP_ALL,
            BYTE_AND_WORD_INTS,
            BYTE_AND_WORD_ALL,
        ];

        let regs_1 = vec![0x1234u16];
        let regs_2 = vec![0x4142u16, 0x4344];
        let regs_4 = vec![0x0102u16, 0x0304, 0x0506, 0x0708];

        let integer_formats = [
            NumFormat::Uint16,
            NumFormat::Int16,
            NumFormat::Uint32,
            NumFormat::Int32,
            NumFormat::Uint64,
            NumFormat::Int64,
        ];

        for &ws in &swap_configs {
            for &nf in &integer_formats {
                let regs = match nf.width() {
                    1 => &regs_1,
                    2 => &regs_2,
                    4 => &regs_4,
                    _ => unreachable!(),
                };
                assert_roundtrip(nf, regs, &ws);
            }
        }
    }

    /// Float round-trip with all swap configs using value-based approach.
    /// Starts from clean decimal values to avoid display precision issues.
    #[test]
    fn roundtrip_float_formats_all_swaps() {
        let swap_configs = [
            NO_SWAP,
            BYTE_SWAP,
            WORD_SWAP_FLOATS,
            WORD_SWAP_ALL,
            BYTE_AND_WORD_FLOATS,
            BYTE_AND_WORD_ALL,
        ];

        for &ws in &swap_configs {
            assert_value_roundtrip(NumFormat::Float16, "1.0", &ws);
            assert_value_roundtrip(NumFormat::Float32, "42.0", &ws);
            assert_value_roundtrip(NumFormat::Float32, "-1.5", &ws);
            assert_value_roundtrip(NumFormat::Float64, "42.0", &ws);
            assert_value_roundtrip(NumFormat::Float64, "-100.25", &ws);
        }
    }

    // =======================================================================
    // 7. Edge cases
    // =======================================================================

    #[test]
    fn format_value_too_few_registers() {
        assert_eq!(NumFormat::Uint32.format_value(&[1], &NO_SWAP), "?");
        assert_eq!(NumFormat::Uint64.format_value(&[1, 2, 3], &NO_SWAP), "?");
        assert_eq!(NumFormat::Uint32.format_value(&[1], &BYTE_SWAP), "?");
    }

    #[test]
    fn byte_swap_zero_is_zero() {
        // 0x0000 byte-swapped is still 0x0000
        assert_eq!(NumFormat::Uint16.format_value(&[0], &BYTE_SWAP), "0");
        assert_eq!(NumFormat::Int16.format_value(&[0], &BYTE_SWAP), "0");
        assert_eq!(NumFormat::Uint32.format_value(&[0, 0], &BYTE_SWAP), "0");
    }

    #[test]
    fn byte_swap_palindrome_unchanged() {
        // 0xAAAA byte-swapped = 0xAAAA (palindromic bytes)
        assert_eq!(
            NumFormat::Uint16.format_value(&[0xAAAA], &BYTE_SWAP),
            NumFormat::Uint16.format_value(&[0xAAAA], &NO_SWAP)
        );
        // 0xFFFF byte-swapped = 0xFFFF
        assert_eq!(
            NumFormat::Uint16.format_value(&[0xFFFF], &BYTE_SWAP),
            NumFormat::Uint16.format_value(&[0xFFFF], &NO_SWAP)
        );
    }

    #[test]
    fn parse_hex_input_with_byte_swap() {
        // "0xFF" = 255 as u16 → byte-swapped for wire → 0xFF00
        let regs = NumFormat::Uint16.parse_value("0xFF", &BYTE_SWAP).unwrap();
        assert_eq!(regs, vec![0xFF00]);
    }

    #[test]
    fn parse_binary_input_with_byte_swap() {
        // "0b100000001" = 257 = 0x0101 → byte-swapped → 0x0101 (palindromic)
        let regs = NumFormat::Uint16
            .parse_value("0b100000001", &BYTE_SWAP)
            .unwrap();
        assert_eq!(regs, vec![0x0101]);
    }

    // =======================================================================
    // 8. should_swap logic
    // =======================================================================

    #[test]
    fn should_swap_single_reg_always_false() {
        for &ws in &[NO_SWAP, WORD_SWAP_INTS, WORD_SWAP_FLOATS, WORD_SWAP_ALL] {
            assert!(!NumFormat::Uint16.should_swap(&ws));
            assert!(!NumFormat::Int16.should_swap(&ws));
            assert!(!NumFormat::Bin16.should_swap(&ws));
            assert!(!NumFormat::Float16.should_swap(&ws));
        }
    }

    #[test]
    fn should_swap_int_types() {
        assert!(NumFormat::Int32.should_swap(&WORD_SWAP_INTS));
        assert!(NumFormat::Uint32.should_swap(&WORD_SWAP_INTS));
        assert!(NumFormat::Int64.should_swap(&WORD_SWAP_INTS));
        assert!(NumFormat::Uint64.should_swap(&WORD_SWAP_INTS));
        assert!(!NumFormat::Int32.should_swap(&WORD_SWAP_FLOATS));
        assert!(!NumFormat::Uint32.should_swap(&WORD_SWAP_FLOATS));
    }

    #[test]
    fn should_swap_float_types() {
        assert!(NumFormat::Float32.should_swap(&WORD_SWAP_FLOATS));
        assert!(NumFormat::Float64.should_swap(&WORD_SWAP_FLOATS));
        assert!(!NumFormat::Float32.should_swap(&WORD_SWAP_INTS));
        assert!(!NumFormat::Float64.should_swap(&WORD_SWAP_INTS));
    }

    // =======================================================================
    // 9. f16 conversion helpers
    // =======================================================================

    #[test]
    fn f16_roundtrip_normal_values() {
        for &val in &[0.0f32, 1.0, -1.0, 0.5, 65504.0] {
            let bits = f32_to_f16(val);
            let back = f16_to_f32(bits);
            assert!(
                (back - val).abs() < 1e-3,
                "f16 roundtrip failed for {}: got {}",
                val,
                back
            );
        }
    }

    #[test]
    fn f16_infinity() {
        let bits = f32_to_f16(f32::INFINITY);
        assert_eq!(bits, 0x7C00);
        let bits_neg = f32_to_f16(f32::NEG_INFINITY);
        assert_eq!(bits_neg, 0xFC00);
    }

    #[test]
    fn f16_nan() {
        let bits = f32_to_f16(f32::NAN);
        // Should be some NaN (exp=31, frac≠0)
        assert_eq!(bits & 0x7C00, 0x7C00);
        assert_ne!(bits & 0x03FF, 0);
    }

    // =======================================================================
    // 10. Concrete Modbus scenario: device with byte-swapped registers
    // =======================================================================

    #[test]
    fn modbus_scenario_byte_swapped_device() {
        // A device stores 32-bit value 305419896 (0x12345678)
        // In standard big-endian: regs [0x1234, 0x5678]
        // If the device byte-swaps each register on the wire: [0x3412, 0x7856]
        //
        // With byte-swap enabled, we read [0x3412, 0x7856]:
        // byte-swap each → [0x1234, 0x5678] → combine → 0x12345678 = 305419896
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x3412, 0x7856], &BYTE_SWAP),
            "305419896"
        );

        // Without byte-swap, the same wire data would be misinterpreted:
        // combine(0x3412, 0x7856) = 0x34127856
        let misinterpreted = ((0x3412u32) << 16) | 0x7856u32;
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x3412, 0x7856], &NO_SWAP),
            misinterpreted.to_string()
        );
    }

    #[test]
    fn modbus_scenario_both_swaps() {
        // A device that both byte-swaps AND word-swaps (DCBA byte order)
        // Value 0x12345678 = 305419896
        // Standard big-endian regs: [0x1234, 0x5678]
        // Word-swapped: [0x5678, 0x1234]
        // Byte-swapped: [0x7856, 0x3412]
        // So on wire we see [0x7856, 0x3412]
        //
        // Reading with both swaps enabled:
        // Step 1 byte-swap: [0x5678, 0x1234]
        // Step 2 word-swap: [0x1234, 0x5678]
        // combine → 0x12345678 = 305419896
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x7856, 0x3412], &BYTE_AND_WORD_INTS),
            "305419896"
        );
    }

    #[test]
    fn modbus_scenario_write_with_byte_swap() {
        // User wants to write 305419896 to a byte-swapped device
        // 305419896 = 0x12345678 → split → [0x1234, 0x5678]
        // byte-swap for wire → [0x3412, 0x7856]
        let regs = NumFormat::Uint32
            .parse_value("305419896", &BYTE_SWAP)
            .unwrap();
        assert_eq!(regs, vec![0x3412, 0x7856]);
    }

    #[test]
    fn modbus_scenario_write_with_both_swaps() {
        // User wants to write 305419896 with both byte+word swap
        // 305419896 = 0x12345678 → split → [0x1234, 0x5678]
        // word-swap → [0x5678, 0x1234]
        // byte-swap → [0x7856, 0x3412]
        let regs = NumFormat::Uint32
            .parse_value("305419896", &BYTE_AND_WORD_INTS)
            .unwrap();
        assert_eq!(regs, vec![0x7856, 0x3412]);
    }

    // =======================================================================
    // ASCII format tests
    // =======================================================================

    #[test]
    fn format_ascii_printable() {
        // 'H' = 0x48, 'i' = 0x69 → register = 0x4869
        assert_eq!(NumFormat::Ascii.format_value(&[0x4869], &NO_SWAP), "Hi");
    }

    #[test]
    fn format_ascii_non_printable_replaced_with_dot() {
        // 0x0048 → high byte = 0x00 (non-printable), low byte = 'H'
        assert_eq!(NumFormat::Ascii.format_value(&[0x0048], &NO_SWAP), ".H");
    }

    #[test]
    fn format_ascii_space_preserved() {
        // ' ' = 0x20, 'A' = 0x41 → 0x2041
        assert_eq!(NumFormat::Ascii.format_value(&[0x2041], &NO_SWAP), " A");
    }

    #[test]
    fn format_ascii_all_non_printable() {
        assert_eq!(NumFormat::Ascii.format_value(&[0x0001], &NO_SWAP), "..");
    }

    #[test]
    fn format_ascii_with_byte_swap() {
        // 0x4869 byte-swapped → 0x6948 → 'i' = 0x69, 'H' = 0x48
        assert_eq!(NumFormat::Ascii.format_value(&[0x4869], &BYTE_SWAP), "iH");
    }

    #[test]
    fn parse_ascii_two_chars() {
        let regs = NumFormat::Ascii.parse_value("Hi", &NO_SWAP).unwrap();
        assert_eq!(regs, vec![0x4869]);
    }

    #[test]
    fn parse_ascii_rejects_wrong_length() {
        assert!(NumFormat::Ascii.parse_value("A", &NO_SWAP).is_err());
        assert!(NumFormat::Ascii.parse_value("ABC", &NO_SWAP).is_err());
        assert!(NumFormat::Ascii.parse_value("", &NO_SWAP).is_err());
    }

    #[test]
    fn ascii_roundtrip_no_swap() {
        assert_roundtrip(NumFormat::Ascii, &[0x4869], &NO_SWAP); // "Hi"
        assert_roundtrip(NumFormat::Ascii, &[0x4142], &NO_SWAP); // "AB"
        assert_roundtrip(NumFormat::Ascii, &[0x3031], &NO_SWAP); // "01"
    }

    #[test]
    fn ascii_roundtrip_byte_swap() {
        assert_roundtrip(NumFormat::Ascii, &[0x4869], &BYTE_SWAP);
        assert_roundtrip(NumFormat::Ascii, &[0x4142], &BYTE_SWAP);
    }

    // =======================================================================
    // swap_words (general word-swap) tests
    // =======================================================================

    #[test]
    fn swap_words_applies_to_uint32() {
        // 0x00010002 = 65538, regs = [0x0001, 0x0002]
        // With swap_words, register order reversed: reads [0x0002, 0x0001] → 0x00020001 = 131073
        assert_eq!(
            NumFormat::Uint32.format_value(&[0x0001, 0x0002], &SWAP_WORDS),
            "131073"
        );
    }

    #[test]
    fn swap_words_applies_to_float32() {
        // 1.0f32 = 0x3F800000, regs = [0x3F80, 0x0000]
        // With swap_words: reversed → [0x0000, 0x3F80] → 0x00003F80
        let no_swap = NumFormat::Float32.format_value(&[0x3F80, 0x0000], &NO_SWAP);
        let swapped = NumFormat::Float32.format_value(&[0x3F80, 0x0000], &SWAP_WORDS);
        assert_ne!(no_swap, swapped, "swap_words should change Float32 interpretation");
        assert_eq!(no_swap, "1.000000");
    }

    #[test]
    fn swap_words_applies_to_int64() {
        // -1 as i64 = all-ones — swap has no effect on all-ones
        assert_eq!(
            NumFormat::Int64.format_value(&[0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF], &SWAP_WORDS),
            "-1"
        );
        // 1 as u64 = [0, 0, 0, 1] → swap 32-bit halves → [0, 1, 0, 0] = 1 << 32
        assert_eq!(
            NumFormat::Uint64.format_value(&[0, 0, 0, 1], &SWAP_WORDS),
            "4294967296"
        );
    }

    #[test]
    fn swap_words_does_not_affect_single_register() {
        // Single-register types are unaffected by swap_words
        assert_eq!(NumFormat::Uint16.format_value(&[0x0102], &SWAP_WORDS), "258");
        assert_eq!(NumFormat::Int16.format_value(&[0x00FF], &SWAP_WORDS), "255");
        assert_eq!(NumFormat::Ascii.format_value(&[0x4869], &SWAP_WORDS), "Hi");
        assert_eq!(
            NumFormat::Bin16.format_value(&[0x0102], &SWAP_WORDS),
            "0000000100000010"
        );
    }

    #[test]
    fn swap_words_roundtrip_uint32() {
        assert_roundtrip(NumFormat::Uint32, &[0x0001, 0x0002], &SWAP_WORDS);
    }

    #[test]
    fn swap_words_roundtrip_float32() {
        assert_value_roundtrip(NumFormat::Float32, "1.0", &SWAP_WORDS);
        assert_value_roundtrip(NumFormat::Float32, "-3.14", &SWAP_WORDS);
    }

    #[test]
    fn swap_words_roundtrip_uint64() {
        assert_roundtrip(NumFormat::Uint64, &[0, 0, 0, 1], &SWAP_WORDS);
    }

    #[test]
    fn swap_words_roundtrip_float64() {
        assert_value_roundtrip(NumFormat::Float64, "1.0", &SWAP_WORDS);
    }

    #[test]
    fn swap_words_with_byte_swap_roundtrip() {
        assert_roundtrip(NumFormat::Uint32, &[0x1234, 0x5678], &SWAP_WORDS_AND_BYTES);
        assert_value_roundtrip(NumFormat::Float32, "1.0", &SWAP_WORDS_AND_BYTES);
        assert_roundtrip(NumFormat::Uint64, &[0x1111, 0x2222, 0x3333, 0x4444], &SWAP_WORDS_AND_BYTES);
    }

    #[test]
    fn swap_words_overrides_individual_flags() {
        // Even with ints=false, floats=false, words=true should swap multi-register types
        let ws = WordSwap {
            ints: false,
            floats: false,
            words: true,
            bytes: false,
        };
        assert!(NumFormat::Float32.should_swap(&ws));
        assert!(NumFormat::Uint32.should_swap(&ws));
        assert!(NumFormat::Int64.should_swap(&ws));
        assert!(NumFormat::Float64.should_swap(&ws));
        // Single-register: never swapped
        assert!(!NumFormat::Uint16.should_swap(&ws));
        assert!(!NumFormat::Int16.should_swap(&ws));
        assert!(!NumFormat::Ascii.should_swap(&ws));
        assert!(!NumFormat::Bin16.should_swap(&ws));
        assert!(!NumFormat::Float16.should_swap(&ws));
    }
}
