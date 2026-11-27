//! #4 颜色转换（`csscolorparser`）。
//!
//! 双向输出 HEX / RGB / HSL / HWB / OKLCH / CMYK，透明度小于 100% 时自动带
//! Alpha 通道，并给出对 `#FFFFFF` 与 `#000000` 的 WCAG 对比度与 AA/AAA 徽章。

use csscolorparser::Color;

use crate::util::invalid_syntax;

use devtools_core::ToolError;

/// 单次颜色转换请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorConverterRequest {
    /// 任意 CSS Color 4 支持的颜色写法。
    pub input: String,
}

/// WCAG 对比度徽章。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContrastReport {
    /// 对比度（1.0–21.0），保留两位小数。
    pub ratio: f64,
    /// 普通文本 AA（≥ 4.5）。
    pub aa: bool,
    /// 普通文本 AAA（≥ 7.0）。
    pub aaa: bool,
    /// 大号文本 AA（≥ 3.0）。
    pub aa_large: bool,
    /// 大号文本 AAA（≥ 4.5）。
    pub aaa_large: bool,
}

/// 颜色转换结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ColorConverterResponse {
    /// `#rrggbb`，透明度小于 1 时为 `#rrggbbaa`。
    pub hex: String,
    /// CSS `rgb()` 表示。
    pub rgb: String,
    pub hsl: String,
    pub hwb: String,
    pub oklch: String,
    pub cmyk: String,
    pub rgb8: [u8; 3],
    /// Alpha，0.0–1.0。
    pub alpha: f64,
    /// Alpha 百分比，0–100。
    pub alpha_percent: f64,
    pub against_white: ContrastReport,
    pub against_black: ContrastReport,
}

/// 解析并转换颜色。
pub fn convert(req: &ColorConverterRequest) -> Result<ColorConverterResponse, ToolError> {
    let input = req.input.trim();
    if input.is_empty() {
        return Err(invalid_syntax("颜色输入为空"));
    }
    let color: Color = csscolorparser::parse(input)
        .map_err(|err| invalid_syntax(format!("无法解析颜色 `{input}`：{err}")))?;
    let [r, g, b, _] = color.to_rgba8();
    let [_, _, _, alpha] = color.to_array();
    let alpha = f64::from(alpha);

    Ok(ColorConverterResponse {
        hex: color.to_css_hex().to_string(),
        rgb: color.to_css_rgb().to_string(),
        hsl: color.to_css_hsl().to_string(),
        hwb: color.to_css_hwb().to_string(),
        oklch: color.to_css_oklch().to_string(),
        cmyk: cmyk_string([r, g, b]),
        rgb8: [r, g, b],
        alpha,
        alpha_percent: alpha * 100.0,
        against_white: contrast_report([r, g, b], alpha, [255, 255, 255]),
        against_black: contrast_report([r, g, b], alpha, [0, 0, 0]),
    })
}

fn cmyk_string([r, g, b]: [u8; 3]) -> String {
    let r = f64::from(r) / 255.0;
    let g = f64::from(g) / 255.0;
    let b = f64::from(b) / 255.0;
    let k = 1.0 - r.max(g).max(b);
    let (c, m, y) = if k >= 1.0 {
        (0.0, 0.0, 0.0)
    } else {
        (
            (1.0 - r - k) / (1.0 - k),
            (1.0 - g - k) / (1.0 - k),
            (1.0 - b - k) / (1.0 - k),
        )
    };
    format!(
        "cmyk({:.0}%, {:.0}%, {:.0}%, {:.0}%)",
        (c * 100.0).round(),
        (m * 100.0).round(),
        (y * 100.0).round(),
        (k * 100.0).round()
    )
}

fn contrast_report(rgb: [u8; 3], alpha: f64, backdrop: [u8; 3]) -> ContrastReport {
    let composited = composite(rgb, alpha, backdrop);
    let ratio = contrast_ratio(relative_luminance(composited), relative_luminance(backdrop));
    let ratio = (ratio * 100.0).round() / 100.0;
    ContrastReport {
        ratio,
        aa: ratio >= 4.5,
        aaa: ratio >= 7.0,
        aa_large: ratio >= 3.0,
        aaa_large: ratio >= 4.5,
    }
}

fn composite(foreground: [u8; 3], alpha: f64, backdrop: [u8; 3]) -> [u8; 3] {
    let mix = |fg: u8, bg: u8| -> u8 {
        (f64::from(fg) * alpha + f64::from(bg) * (1.0 - alpha))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [
        mix(foreground[0], backdrop[0]),
        mix(foreground[1], backdrop[1]),
        mix(foreground[2], backdrop[2]),
    ]
}

/// WCAG 2.x 相对亮度。
pub fn relative_luminance([r, g, b]: [u8; 3]) -> f64 {
    let channel = |value: u8| -> f64 {
        let s = f64::from(value) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG 对比度，取值 1.0–21.0。
pub fn contrast_ratio(luminance_a: f64, luminance_b: f64) -> f64 {
    let (lighter, darker) = if luminance_a >= luminance_b {
        (luminance_a, luminance_b)
    } else {
        (luminance_b, luminance_a)
    };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn convert_color(input: &str) -> ColorConverterResponse {
        convert(&ColorConverterRequest {
            input: input.to_string(),
        })
        .unwrap()
    }

    #[test]
    fn hex_shorthand_normalises() {
        let resp = convert_color("#fff");
        assert_eq!(resp.hex, "#ffffff");
        assert_eq!(resp.rgb8, [255, 255, 255]);
        assert_eq!(resp.alpha, 1.0);
    }

    #[test]
    fn named_color_parses() {
        let resp = convert_color("red");
        assert_eq!(resp.hex, "#ff0000");
        assert_eq!(resp.rgb, "rgb(255 0 0)");
        assert_eq!(resp.hsl, "hsl(0 100% 50%)");
        assert_eq!(resp.cmyk, "cmyk(0%, 100%, 100%, 0%)");
    }

    #[test]
    fn alpha_channel_is_reported_below_full_opacity() {
        let resp = convert_color("rgba(0, 0, 0, 0.5)");
        assert_eq!(resp.hex, "#00000080");
        assert!((resp.alpha_percent - 50.0).abs() < 0.01);
        assert!(resp.rgb.contains('/'), "{}", resp.rgb);
    }

    #[test]
    fn oklch_input_round_trips_to_rgb() {
        let resp = convert_color("oklch(0.7 0.15 200)");
        assert!(resp.hex.starts_with('#'), "{}", resp.hex);
        assert!(resp.oklch.starts_with("oklch("), "{}", resp.oklch);
    }

    #[test]
    fn black_on_white_has_maximum_contrast() {
        let resp = convert_color("#000000");
        assert!((resp.against_white.ratio - 21.0).abs() < 0.01);
        assert!(resp.against_white.aaa);
        assert!(resp.against_white.aa);
        assert!(!resp.against_black.aa_large);
        assert_eq!(resp.against_black.ratio, 1.0);
    }

    #[test]
    fn mid_grey_fails_normal_text_aa() {
        let resp = convert_color("#767676");
        assert!(resp.against_white.aa);
        assert!(!resp.against_white.aaa);
    }

    #[test]
    fn transparent_color_is_composited_over_backdrop() {
        let resp = convert_color("rgba(255, 255, 255, 0.0)");
        // 完全透明 → 与背景同色 → 对比度 1。
        assert_eq!(resp.against_white.ratio, 1.0);
        assert_eq!(resp.against_black.ratio, 1.0);
    }

    #[test]
    fn invalid_color_is_rejected() {
        let err = convert(&ColorConverterRequest {
            input: "not-a-color".to_string(),
        })
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }
}
