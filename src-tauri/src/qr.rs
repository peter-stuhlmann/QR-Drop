//! QR code as inline SVG for the share window.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

/// Renders `data` as an SVG string without quiet zone (the UI adds the white
/// margin around it) and without fixed size, so CSS controls the dimensions.
pub fn svg(data: &str) -> Result<String, qrcode::types::QrError> {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M)?;
    let raw = code
        .render::<svg::Color>()
        .quiet_zone(false)
        .module_dimensions(1, 1)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(strip_header(&raw))
}

/// Drops the XML declaration and the fixed width/height of the opening
/// `<svg>` tag (the viewBox keeps the aspect ratio). Attributes of inner
/// elements, like the background `<rect width="100%">`, stay untouched.
fn strip_header(raw: &str) -> String {
    let start = raw.find("<svg").unwrap_or(0);
    let svg = &raw[start..];
    let tag_end = svg.find('>').map_or(svg.len(), |i| i + 1);
    let (mut tag, body) = (svg[..tag_end].to_string(), &svg[tag_end..]);
    for attr in [" width=\"", " height=\""] {
        if let Some(pos) = tag.find(attr) {
            if let Some(len) = tag[pos + attr.len()..].find('"') {
                tag.replace_range(pos..pos + attr.len() + len + 1, "");
            }
        }
    }
    tag + body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_scalable_svg() {
        let s = svg("http://192.168.178.24:52817/s/Xq7hK2pLw9vN4mRtY8bZcA").unwrap();
        assert!(s.starts_with("<svg"));
        let svg_tag = &s[..s.find('>').unwrap()];
        assert!(svg_tag.contains("viewBox=\"0 0 33 33\""));
        assert!(!svg_tag.contains(" width=\""));
        assert!(!svg_tag.contains(" height=\""));
        assert!(s.contains("shape-rendering=\"crispEdges\""));
        assert!(s.ends_with("</svg>"));
    }

    #[test]
    fn only_the_svg_tag_loses_its_size() {
        let raw = "<?xml version=\"1.0\"?><svg width=\"33\" height=\"33\" viewBox=\"0 0 33 33\"><rect width=\"100%\" height=\"100%\"/></svg>";
        assert_eq!(
            strip_header(raw),
            "<svg viewBox=\"0 0 33 33\"><rect width=\"100%\" height=\"100%\"/></svg>"
        );
    }
}
