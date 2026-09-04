use gpui::{KeyDownEvent, Keystroke};

pub(crate) fn encode_event(event: &KeyDownEvent, app_cursor: bool) -> Option<Vec<u8>> {
    if event.prefer_character_input {
        return None;
    }

    encode(&event.keystroke, app_cursor)
}

pub(crate) fn encode(keystroke: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform || modifiers.function {
        return None;
    }

    let key = keystroke.key.as_str();
    let modified = modifiers.shift || modifiers.alt || modifiers.control;

    if let Some(final_byte) = navigation_final(key) {
        if modified {
            return Some(format!("\x1b[1;{}{final_byte}", modifier_code(keystroke)).into_bytes());
        }
        let introducer = if app_cursor { "\x1bO" } else { "\x1b[" };
        return Some(format!("{introducer}{final_byte}").into_bytes());
    }

    if let Some(final_byte) = f1_to_f4_final(key) {
        if modified {
            return Some(format!("\x1b[1;{}{final_byte}", modifier_code(keystroke)).into_bytes());
        }
        return Some(format!("\x1bO{final_byte}").into_bytes());
    }

    if let Some(parameter) = tilde_parameter(key) {
        if modified {
            return Some(format!("\x1b[{parameter};{}~", modifier_code(keystroke)).into_bytes());
        }
        return Some(format!("\x1b[{parameter}~").into_bytes());
    }

    match key {
        "tab" if !modified => return Some(vec![b'\t']),
        "tab" if modifiers.shift && !modifiers.alt && !modifiers.control => {
            return Some(b"\x1b[Z".to_vec());
        }
        "escape" if !modified => return Some(vec![0x1b]),
        "escape" if modifiers.alt && !modifiers.shift && !modifiers.control => {
            return Some(vec![0x1b, 0x1b]);
        }
        "enter" if !modified => return Some(vec![b'\r']),
        "enter" if modifiers.shift && !modifiers.alt && !modifiers.control => {
            return Some(vec![b'\n']);
        }
        "enter" if modifiers.alt && !modifiers.shift && !modifiers.control => {
            return Some(vec![0x1b, b'\r']);
        }
        "backspace" => {
            let byte = if modifiers.control { 0x08 } else { 0x7f };
            return Some(if modifiers.alt {
                vec![0x1b, byte]
            } else {
                vec![byte]
            });
        }
        _ => {}
    }

    if modifiers.control {
        let byte = keystroke
            .key_char
            .as_deref()
            .and_then(control_byte)
            .or_else(|| control_byte(key))?;
        return Some(if modifiers.alt {
            vec![0x1b, byte]
        } else {
            vec![byte]
        });
    }

    if modifiers.alt {
        return meta_bytes(keystroke);
    }

    None
}

fn navigation_final(key: &str) -> Option<char> {
    match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    }
}

fn f1_to_f4_final(key: &str) -> Option<char> {
    match key {
        "f1" => Some('P'),
        "f2" => Some('Q'),
        "f3" => Some('R'),
        "f4" => Some('S'),
        _ => None,
    }
}

fn tilde_parameter(key: &str) -> Option<u8> {
    match key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        "f13" => Some(25),
        "f14" => Some(26),
        "f15" => Some(28),
        "f16" => Some(29),
        "f17" => Some(31),
        "f18" => Some(32),
        "f19" => Some(33),
        "f20" => Some(34),
        _ => None,
    }
}

fn modifier_code(keystroke: &Keystroke) -> u8 {
    1 + u8::from(keystroke.modifiers.shift)
        + 2 * u8::from(keystroke.modifiers.alt)
        + 4 * u8::from(keystroke.modifiers.control)
}

fn control_byte(key: &str) -> Option<u8> {
    if key.len() == 1 {
        let byte = key.as_bytes()[0];
        if byte.is_ascii_alphabetic() {
            return Some(byte.to_ascii_uppercase() - b'@');
        }
    }

    match key {
        "space" | "@" | "`" | "2" => Some(0x00),
        "[" | "3" => Some(0x1b),
        "\\" | "4" => Some(0x1c),
        "]" | "5" => Some(0x1d),
        "^" | "6" => Some(0x1e),
        "_" | "/" | "7" => Some(0x1f),
        "?" | "8" => Some(0x7f),
        _ => None,
    }
}

fn meta_bytes(keystroke: &Keystroke) -> Option<Vec<u8>> {
    let text = if let Some(text) = keystroke
        .key_char
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        text.to_string()
    } else if keystroke.key.chars().count() == 1 {
        if keystroke.modifiers.shift {
            keystroke.key.to_uppercase()
        } else {
            keystroke.key.clone()
        }
    } else {
        return None;
    };

    let mut bytes = Vec::with_capacity(1 + text.len());
    bytes.push(0x1b);
    bytes.extend_from_slice(text.as_bytes());
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn key(key: &str) -> Keystroke {
        keystroke(key, None, Modifiers::default())
    }

    fn modified(key: &str, shift: bool, alt: bool, control: bool) -> Keystroke {
        keystroke(
            key,
            None,
            Modifiers {
                shift,
                alt,
                control,
                ..Default::default()
            },
        )
    }

    fn keystroke(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.into(),
            key_char: key_char.map(Into::into),
        }
    }

    fn bytes(value: &str) -> Option<Vec<u8>> {
        Some(value.as_bytes().to_vec())
    }

    #[test]
    fn navigation_uses_cursor_mode_only_without_modifiers() {
        for (key_name, final_byte) in [
            ("up", "A"),
            ("down", "B"),
            ("right", "C"),
            ("left", "D"),
            ("home", "H"),
            ("end", "F"),
        ] {
            assert_eq!(
                encode(&key(key_name), false),
                bytes(&format!("\x1b[{final_byte}"))
            );
            assert_eq!(
                encode(&key(key_name), true),
                bytes(&format!("\x1bO{final_byte}"))
            );
        }
    }

    #[test]
    fn navigation_modifiers_use_xterm_codes() {
        assert_eq!(
            encode(&modified("up", false, true, false), true),
            bytes("\x1b[1;3A")
        );
        assert_eq!(
            encode(&modified("left", true, false, false), false),
            bytes("\x1b[1;2D")
        );
        assert_eq!(
            encode(&modified("right", false, false, true), false),
            bytes("\x1b[1;5C")
        );
        assert_eq!(
            encode(&modified("down", false, true, true), false),
            bytes("\x1b[1;7B")
        );
        assert_eq!(
            encode(&modified("home", true, true, false), true),
            bytes("\x1b[1;4H")
        );
        assert_eq!(
            encode(&modified("end", true, false, true), true),
            bytes("\x1b[1;6F")
        );
    }

    #[test]
    fn unclaimed_alt_arrows_keep_terminal_fallback_sequences() {
        for (key_name, expected) in [
            ("up", "\x1b[1;3A"),
            ("down", "\x1b[1;3B"),
            ("right", "\x1b[1;3C"),
            ("left", "\x1b[1;3D"),
        ] {
            assert_eq!(
                encode(&modified(key_name, false, true, false), false),
                bytes(expected)
            );
        }
    }

    #[test]
    fn insert_delete_and_page_keys_support_standard_modifiers() {
        for (key_name, parameter) in [("insert", 2), ("delete", 3), ("pageup", 5), ("pagedown", 6)]
        {
            assert_eq!(
                encode(&key(key_name), false),
                bytes(&format!("\x1b[{parameter}~"))
            );
            assert_eq!(
                encode(&modified(key_name, true, false, false), false),
                bytes(&format!("\x1b[{parameter};2~"))
            );
            assert_eq!(
                encode(&modified(key_name, false, true, false), false),
                bytes(&format!("\x1b[{parameter};3~"))
            );
            assert_eq!(
                encode(&modified(key_name, false, false, true), false),
                bytes(&format!("\x1b[{parameter};5~"))
            );
        }
    }

    #[test]
    fn function_keys_f1_through_f20_support_standard_modifiers() {
        let parameters = [
            15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34,
        ];
        for (index, final_byte) in ['P', 'Q', 'R', 'S'].into_iter().enumerate() {
            let key_name = format!("f{}", index + 1);
            assert_eq!(
                encode(&key(&key_name), false),
                bytes(&format!("\x1bO{final_byte}"))
            );
            assert_eq!(
                encode(&modified(&key_name, true, false, false), false),
                bytes(&format!("\x1b[1;2{final_byte}"))
            );
            assert_eq!(
                encode(&modified(&key_name, false, true, false), false),
                bytes(&format!("\x1b[1;3{final_byte}"))
            );
            assert_eq!(
                encode(&modified(&key_name, false, false, true), false),
                bytes(&format!("\x1b[1;5{final_byte}"))
            );
        }
        for (offset, parameter) in parameters.into_iter().enumerate() {
            let key_name = format!("f{}", offset + 5);
            assert_eq!(
                encode(&key(&key_name), false),
                bytes(&format!("\x1b[{parameter}~"))
            );
            assert_eq!(
                encode(&modified(&key_name, true, false, false), false),
                bytes(&format!("\x1b[{parameter};2~"))
            );
            assert_eq!(
                encode(&modified(&key_name, false, true, false), false),
                bytes(&format!("\x1b[{parameter};3~"))
            );
            assert_eq!(
                encode(&modified(&key_name, false, false, true), false),
                bytes(&format!("\x1b[{parameter};5~"))
            );
        }

        assert_eq!(encode(&key("f1"), false), bytes("\x1bOP"));
        assert_eq!(encode(&key("f5"), false), bytes("\x1b[15~"));
        assert_eq!(encode(&key("f12"), false), bytes("\x1b[24~"));
        assert_eq!(encode(&key("f20"), false), bytes("\x1b[34~"));
    }

    #[test]
    fn control_ascii_and_alt_meta_are_preserved() {
        assert_eq!(
            encode(&modified("a", false, false, true), false),
            Some(vec![0x01])
        );
        assert_eq!(
            encode(&modified("k", false, false, true), false),
            Some(vec![0x0b])
        );
        assert_eq!(
            encode(&modified("w", false, false, true), false),
            Some(vec![0x17])
        );
        assert_eq!(
            encode(&modified("t", true, false, true), false),
            Some(vec![0x14])
        );
        assert_eq!(
            encode(&modified("Z", true, false, true), false),
            Some(vec![0x1a])
        );
        for (key_name, expected) in [
            ("space", 0x00),
            ("@", 0x00),
            ("`", 0x00),
            ("2", 0x00),
            ("[", 0x1b),
            ("3", 0x1b),
            ("\\", 0x1c),
            ("4", 0x1c),
            ("]", 0x1d),
            ("5", 0x1d),
            ("^", 0x1e),
            ("6", 0x1e),
            ("_", 0x1f),
            ("/", 0x1f),
            ("7", 0x1f),
            ("?", 0x7f),
            ("8", 0x7f),
        ] {
            assert_eq!(
                encode(&modified(key_name, false, false, true), false),
                Some(vec![expected])
            );
        }
        assert_eq!(
            encode(&modified("c", false, true, true), false),
            Some(vec![0x1b, 0x03])
        );
        assert_eq!(
            encode(&modified("a", false, true, false), false),
            bytes("\x1ba")
        );
        assert_eq!(
            encode(&modified("a", true, true, false), false),
            bytes("\x1bA")
        );
        assert_eq!(
            encode(&modified("?", false, true, false), false),
            bytes("\x1b?")
        );
    }

    #[test]
    fn basic_special_keys_keep_common_terminal_semantics() {
        assert_eq!(encode(&key("enter"), false), Some(vec![b'\r']));
        assert_eq!(
            encode(&modified("enter", true, false, false), false),
            Some(vec![b'\n'])
        );
        assert_eq!(
            encode(&modified("enter", false, true, false), false),
            Some(vec![0x1b, b'\r'])
        );
        assert_eq!(encode(&key("tab"), false), Some(vec![b'\t']));
        assert_eq!(
            encode(&modified("tab", true, false, false), false),
            bytes("\x1b[Z")
        );
        assert_eq!(encode(&key("escape"), false), Some(vec![0x1b]));
        assert_eq!(
            encode(&modified("escape", false, true, false), false),
            Some(vec![0x1b, 0x1b])
        );
        assert_eq!(encode(&key("backspace"), false), Some(vec![0x7f]));
        assert_eq!(
            encode(&modified("backspace", true, false, false), false),
            Some(vec![0x7f])
        );
        assert_eq!(
            encode(&modified("backspace", false, true, false), false),
            Some(vec![0x1b, 0x7f])
        );
        assert_eq!(
            encode(&modified("backspace", false, false, true), false),
            Some(vec![0x08])
        );
        assert_eq!(
            encode(&modified("backspace", false, true, true), false),
            Some(vec![0x1b, 0x08])
        );
    }

    #[test]
    fn printable_and_unknown_keys_are_left_for_text_input() {
        assert_eq!(
            encode(&keystroke("a", Some("a"), Modifiers::default()), false),
            None
        );
        assert_eq!(
            encode(&keystroke("s", Some("ß"), Modifiers::default()), false),
            None
        );
        assert_eq!(encode(&key("inserted-key-name"), false), None);
        assert_eq!(encode(&key("f21"), false), None);
        assert_eq!(
            encode(&modified("ß", false, true, false), false),
            bytes("\x1bß")
        );
        assert_eq!(
            encode(
                &keystroke(
                    "s",
                    Some("ß"),
                    Modifiers {
                        alt: true,
                        ..Default::default()
                    },
                ),
                false,
            ),
            bytes("\x1bß")
        );
    }

    #[test]
    fn preferred_character_input_bypasses_terminal_key_encoding() {
        let event = KeyDownEvent {
            keystroke: keystroke(
                "q",
                Some("@"),
                Modifiers {
                    control: true,
                    alt: true,
                    ..Default::default()
                },
            ),
            is_held: false,
            prefer_character_input: true,
        };

        assert_eq!(encode_event(&event, false), None);
    }

    #[test]
    fn unsupported_modifiers_are_not_encoded_as_xterm_modifiers() {
        let platform = keystroke(
            "up",
            None,
            Modifiers {
                platform: true,
                ..Default::default()
            },
        );
        let function = keystroke(
            "f1",
            None,
            Modifiers {
                function: true,
                ..Default::default()
            },
        );
        assert_eq!(encode(&platform, false), None);
        assert_eq!(encode(&function, false), None);
    }
}
