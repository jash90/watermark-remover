pub mod tiling;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    X2,
    X4,
    X8,
}

impl Scale {
    pub fn factor(self) -> u32 {
        match self {
            Scale::X2 => 2,
            Scale::X4 => 4,
            Scale::X8 => 8,
        }
    }
}

/// One step of an upscale: the native ×4 model, or a Lanczos ×0.5 downscale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Model,
    Half,
}

/// ×2 = ×4 then ×0.5 (like Real-ESRGAN's --outscale); ×8 = ×4, ×0.5, ×4, so the
/// second model pass runs on 4× the input pixels instead of 16×.
pub fn passes(scale: Scale) -> Vec<Pass> {
    match scale {
        Scale::X2 => vec![Pass::Model, Pass::Half],
        Scale::X4 => vec![Pass::Model],
        Scale::X8 => vec![Pass::Model, Pass::Half, Pass::Model],
    }
}

pub const MAX_OUTPUT_PIXELS: u64 = 16384 * 16384;

pub fn output_size(w: u32, h: u32, scale: Scale) -> Result<(u32, u32), String> {
    let (ow, oh) = (w as u64 * scale.factor() as u64, h as u64 * scale.factor() as u64);
    if ow * oh > MAX_OUTPUT_PIXELS || ow > u32::MAX as u64 || oh > u32::MAX as u64 {
        return Err(format!(
            "The result would be {}x{} ({} MP). The limit is 256 MP. Choose a smaller scale.",
            ow,
            oh,
            ow * oh / 1_000_000
        ));
    }
    Ok((ow as u32, oh as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_plans() {
        assert_eq!(passes(Scale::X2), vec![Pass::Model, Pass::Half]);
        assert_eq!(passes(Scale::X4), vec![Pass::Model]);
        assert_eq!(passes(Scale::X8), vec![Pass::Model, Pass::Half, Pass::Model]);
    }

    #[test]
    fn output_sizes() {
        assert_eq!(output_size(1600, 900, Scale::X2).unwrap(), (3200, 1800));
        assert_eq!(output_size(1600, 900, Scale::X8).unwrap(), (12800, 7200));
    }

    #[test]
    fn refuses_outputs_over_256_mp() {
        let err = output_size(4000, 3000, Scale::X8).unwrap_err();
        assert!(err.contains("32000x24000"), "{err}");
    }
}
