//! Turning a file name from the phone into a harmless local one.

use crate::share::FALLBACK_NAME;

/// Longest name in UTF-8 bytes. Fits macOS (255 bytes) and Windows (255
/// UTF-16 units, never more than UTF-8 bytes) with room for a " (123)"
/// suffix when the name is taken.
const MAX_NAME_BYTES: usize = 200;

/// Longer "extensions" are just part of the name.
const MAX_EXTENSION_CHARS: usize = 16;

/// Device names Windows reserves in every folder, whatever the extension
/// (including COM0/LPT0 and the superscript digits it also accepts).
const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Turns a name from the phone into a harmless file name: only the last path
/// component, no characters Windows or macOS reject, no invisible
/// formatting characters, no hidden files, no device names, limited length
/// (the extension is kept).
pub(super) fn sanitize_file_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !is_invisible_format(*c))
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Leading dots would hide the file (and "." / ".." are no names);
    // Windows drops trailing dots and spaces on its own.
    let name = cleaned
        .trim_start_matches(['.', ' '])
        .trim_end_matches(['.', ' ']);
    if name.is_empty() {
        return FALLBACK_NAME.to_string();
    }

    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.chars().count() <= MAX_EXTENSION_CHARS => {
            (stem, format!(".{ext}"))
        }
        _ => (name, String::new()),
    };
    let device = stem.split('.').next().unwrap_or_default().trim_end();
    let stem = if WINDOWS_RESERVED
        .iter()
        .any(|r| r.eq_ignore_ascii_case(device))
    {
        format!("_{stem}")
    } else {
        stem.to_string()
    };
    let stem = truncate_bytes(&stem, MAX_NAME_BYTES.saturating_sub(ext.len()));
    format!("{}{ext}", stem.trim_end())
}

/// The longest prefix of `s` with at most `max` bytes, cut at a character
/// boundary.
fn truncate_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Invisible formatting characters (Unicode category Cf) that can disguise a
/// name, e.g. U+202E RIGHT-TO-LEFT OVERRIDE showing "photo" + U+202E + "gpj.exe" as
/// "photoexe.jpg". The zero-width (non-)joiners U+200C/U+200D and the tag
/// characters U+E0020–E007F stay: emoji and several scripts need them.
fn is_invisible_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B
            | 0x200E..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(code_points: &[u32]) -> String {
        code_points
            .iter()
            .map(|&c| char::from_u32(c).unwrap())
            .collect()
    }

    #[test]
    fn names_are_reduced_to_one_safe_component() {
        assert_eq!(sanitize_file_name("IMG_2041.HEIC"), "IMG_2041.HEIC");
        assert_eq!(sanitize_file_name("Urlaub Ä 😀.jpg"), "Urlaub Ä 😀.jpg");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("..\\..\\Windows\\win.ini"), "win.ini");
        assert_eq!(sanitize_file_name("C:\\Users\\x\\a.txt"), "a.txt");
        assert_eq!(
            sanitize_file_name("a<b>c:d\"e|f?g*h.txt"),
            "a_b_c_d_e_f_g_h.txt"
        );
        assert_eq!(sanitize_file_name("tab\there\n.txt"), "tab_here_.txt");
        assert_eq!(sanitize_file_name(".bashrc"), "bashrc");
        assert_eq!(sanitize_file_name("name. . ."), "name");
        assert_eq!(sanitize_file_name(".."), FALLBACK_NAME);
        assert_eq!(sanitize_file_name(""), FALLBACK_NAME);
        assert_eq!(sanitize_file_name("/"), FALLBACK_NAME);
    }

    #[test]
    fn device_names_are_defused() {
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("nul"), "_nul");
        assert_eq!(sanitize_file_name("com1.tar.gz"), "_com1.tar.gz");
        assert_eq!(sanitize_file_name("COM0.log"), "_COM0.log");
        assert_eq!(sanitize_file_name("lpt¹.txt"), "_lpt¹.txt");
        assert_eq!(sanitize_file_name("CONIN$"), "_CONIN$");
        assert_eq!(sanitize_file_name("conout$.txt"), "_conout$.txt");
        assert_eq!(sanitize_file_name("CONSOLE.txt"), "CONSOLE.txt");
        assert_eq!(sanitize_file_name("COM10.txt"), "COM10.txt");
    }

    #[test]
    fn invisible_formatting_is_removed() {
        // "photo" RLO "gpj.exe" would display as "photoexe.jpg".
        let spoofed = format!("photo{}gpj.exe", with(&[0x202E]));
        assert_eq!(sanitize_file_name(&spoofed), "photogpj.exe");
        let hidden = format!(
            "{}a{}b{}.txt",
            with(&[0xFEFF]),
            with(&[0x200B]),
            with(&[0x2066])
        );
        assert_eq!(sanitize_file_name(&hidden), "ab.txt");
        // Family emoji: joined with U+200D, which stays.
        let family = with(&[0x1F468, 0x200D, 0x1F469, 0x200D, 0x1F467]);
        assert_eq!(
            sanitize_file_name(&format!("{family}.png")),
            format!("{family}.png")
        );
    }

    #[test]
    fn long_names_fit_every_file_system_and_keep_their_extension() {
        let long = format!("{}.jpeg", "a".repeat(400));
        let s = sanitize_file_name(&long);
        assert_eq!(s.len(), MAX_NAME_BYTES);
        assert!(s.ends_with(".jpeg"));

        let emoji = format!("{}.png", "😀".repeat(300));
        let s = sanitize_file_name(&emoji);
        assert!(s.len() <= MAX_NAME_BYTES && s.ends_with(".png"), "{s}");
        assert!(s.encode_utf16().count() <= 255);
        // Cut at a character boundary: only whole emoji remain.
        assert!(s.trim_end_matches(".png").chars().all(|c| c == '😀'));
    }
}
