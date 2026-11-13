// SPDX-License-Identifier: MPL-2.0
//! Small helpers: URL encoding, image templates, VOD durations.

use std::time::Duration;

/// Percent-encodes a query component (RFC 3986 unreserved set kept as is).
pub(crate) fn encode_query_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Fills a Helix image template: `{width}`/`{height}` (stream and VOD
/// thumbnails, box art) and `%{width}`/`%{height}` (VOD thumbnails use this
/// form).
///
/// ```
/// let url = pa_twitch::fill_image_template(
///     "https://static-cdn.jtvnw.net/ttv-boxart/33214-{width}x{height}.jpg",
///     285,
///     380,
/// );
/// assert_eq!(url, "https://static-cdn.jtvnw.net/ttv-boxart/33214-285x380.jpg");
/// ```
pub fn fill_image_template(template: &str, width: u32, height: u32) -> String {
    let (w, h) = (width.to_string(), height.to_string());
    template
        .replace("%{width}", &w)
        .replace("%{height}", &h)
        .replace("{width}", &w)
        .replace("{height}", &h)
}

/// Parses a Helix video duration such as `3h8m33s`, `52m4s` or `21s`.
///
/// ```
/// use std::time::Duration;
/// assert_eq!(
///     pa_twitch::parse_video_duration("1h2m3s"),
///     Some(Duration::from_secs(3723))
/// );
/// assert_eq!(pa_twitch::parse_video_duration("oops"), None);
/// ```
pub fn parse_video_duration(text: &str) -> Option<Duration> {
    let mut total: u64 = 0;
    let mut number: Option<u64> = None;
    let mut seen_unit = false;
    for ch in text.trim().chars() {
        if let Some(digit) = ch.to_digit(10) {
            number = Some(number.unwrap_or(0).checked_mul(10)?.checked_add(digit as u64)?);
            continue;
        }
        let factor = match ch {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        total = total.checked_add(number.take()?.checked_mul(factor)?)?;
        seen_unit = true;
    }
    if number.is_some() || !seen_unit {
        return None;
    }
    Some(Duration::from_secs(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_characters() {
        assert_eq!(encode_query_component("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode_query_component("Just_Chatting-1.~"), "Just_Chatting-1.~");
    }

    #[test]
    fn durations() {
        assert_eq!(parse_video_duration("3h8m33s"), Some(Duration::from_secs(11313)));
        assert_eq!(parse_video_duration("52m4s"), Some(Duration::from_secs(3124)));
        assert_eq!(parse_video_duration("21s"), Some(Duration::from_secs(21)));
        assert_eq!(parse_video_duration(""), None);
        assert_eq!(parse_video_duration("12"), None);
        assert_eq!(parse_video_duration("1h2"), None);
        assert_eq!(parse_video_duration("h"), None);
    }

    #[test]
    fn vod_thumbnail_template() {
        assert_eq!(
            fill_image_template("https://x/thumb/%{width}x%{height}.jpg", 320, 180),
            "https://x/thumb/320x180.jpg"
        );
    }
}
