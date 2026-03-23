use std::fmt;

/// Word-swap configuration for multi-register conversions.
#[derive(Debug, Clone, Copy, Default)]
pub struct WordSwap {
    pub ints: bool,
    pub floats: bool,
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
            _ => Err(format!(
                "unknown format '{}'; expected one of: u16, i16, u32, i32, u64, i64, f32, f64, b16",
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
    ];

    /// How many consecutive u16 registers this format consumes.
    pub fn width(self) -> usize {
        match self {
            Self::Int16 | Self::Uint16 | Self::Float16 | Self::Bin16 => 1,
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
        }
    }

    /// Whether this format should have its words swapped given the swap config.
    pub fn should_swap(self, ws: &WordSwap) -> bool {
        if self.width() < 2 {
            return false;
        }
        match self {
            Self::Int32 | Self::Uint32 | Self::Int64 | Self::Uint64 => ws.ints,
            Self::Float32 | Self::Float64 => ws.floats,
            _ => false,
        }
    }

    /// Convert a slice of registers to a display string.
    /// If `swap` config applies, word order is reversed before conversion.
    pub fn format_value(self, regs: &[u16], ws: &WordSwap) -> String {
        if regs.len() < self.width() {
            return "?".to_string();
        }
        // Apply word swap if configured for this format type
        let r: Vec<u16> = if self.should_swap(ws) {
            let mut v = regs[..self.width()].to_vec();
            v.reverse();
            v
        } else {
            regs[..self.width()].to_vec()
        };
        match self {
            Self::Int16 => format!("{}", r[0] as i16),
            Self::Uint16 => format!("{}", r[0]),
            Self::Bin16 => format!("{:016b}", r[0]),
            Self::Float16 => format!("{:.4}", f16_to_f32(r[0])),

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
        // Reverse word order for write if swap is active
        if self.should_swap(ws) {
            regs.reverse();
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

fn f32_to_f16(val: f32) -> u16 {
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

fn f16_to_f32(bits: u16) -> f32 {
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
