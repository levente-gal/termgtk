//! Limited terminal key encoding for Herdr `terminal.input`.
//!
//! Printable text uses the `text` field. Controls use `bytes` (standard base64
//! on the wire). Compose, IME, mouse, and key-release are out of scope.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyInput {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Map one key press into a Herdr input record body.
///
/// Returns `None` for the application shortcut (Ctrl+Shift), Alt/chords, and
/// keys outside the supported set so the caller can leave them to GTK.
pub fn encode_key(event: KeyEvent) -> Option<KeyInput> {
    if event.alt || (event.ctrl && event.shift) {
        return None;
    }
    if event.ctrl {
        let byte = match event.key {
            Key::Char('c' | 'C') => 0x03,
            Key::Char('d' | 'D') => 0x04,
            Key::Char('l' | 'L') => 0x0c,
            Key::Char('r' | 'R') => 0x12,
            _ => return None,
        };
        return Some(KeyInput::Bytes(vec![byte]));
    }
    if event.shift && matches!(event.key, Key::Tab) {
        return None;
    }
    match event.key {
        Key::Char(ch) if !ch.is_control() => Some(KeyInput::Text(ch.to_string())),
        Key::Enter => Some(KeyInput::Bytes(b"\r".to_vec())),
        Key::Backspace => Some(KeyInput::Bytes(vec![0x7f])),
        Key::Tab => Some(KeyInput::Bytes(b"\t".to_vec())),
        Key::Escape => Some(KeyInput::Bytes(vec![0x1b])),
        Key::Up => Some(KeyInput::Bytes(b"\x1b[A".to_vec())),
        Key::Down => Some(KeyInput::Bytes(b"\x1b[B".to_vec())),
        Key::Right => Some(KeyInput::Bytes(b"\x1b[C".to_vec())),
        Key::Left => Some(KeyInput::Bytes(b"\x1b[D".to_vec())),
        Key::Char(_) | Key::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn press(key: Key, ctrl: bool, alt: bool, shift: bool) -> KeyEvent {
        KeyEvent {
            key,
            ctrl,
            alt,
            shift,
        }
    }

    #[test]
    fn printable_text_and_required_controls() {
        assert_eq!(
            encode_key(press(Key::Char('a'), false, false, false)),
            Some(KeyInput::Text("a".into()))
        );
        assert_eq!(
            encode_key(press(Key::Char('A'), false, false, true)),
            Some(KeyInput::Text("A".into()))
        );
        assert_eq!(
            encode_key(press(Key::Enter, false, false, false)),
            Some(KeyInput::Bytes(b"\r".to_vec()))
        );
        assert_eq!(
            encode_key(press(Key::Backspace, false, false, false)),
            Some(KeyInput::Bytes(vec![0x7f]))
        );
        assert_eq!(
            encode_key(press(Key::Tab, false, false, false)),
            Some(KeyInput::Bytes(b"\t".to_vec()))
        );
        assert_eq!(
            encode_key(press(Key::Escape, false, false, false)),
            Some(KeyInput::Bytes(vec![0x1b]))
        );
    }

    #[test]
    fn arrows_match_control_fixture_bytes() {
        let Some(KeyInput::Bytes(bytes)) = encode_key(press(Key::Up, false, false, false)) else {
            panic!("up");
        };
        assert_eq!(bytes, b"\x1b[A");
        assert_eq!(
            base64::engine::general_purpose::STANDARD.encode(bytes),
            "G1tB"
        );
        assert_eq!(
            encode_key(press(Key::Down, false, false, false)),
            Some(KeyInput::Bytes(b"\x1b[B".to_vec()))
        );
        assert_eq!(
            encode_key(press(Key::Right, false, false, false)),
            Some(KeyInput::Bytes(b"\x1b[C".to_vec()))
        );
        assert_eq!(
            encode_key(press(Key::Left, false, false, false)),
            Some(KeyInput::Bytes(b"\x1b[D".to_vec()))
        );
    }

    #[test]
    fn ctrl_letters_and_app_shortcut_stay_out() {
        assert_eq!(
            encode_key(press(Key::Char('c'), true, false, false)),
            Some(KeyInput::Bytes(vec![0x03]))
        );
        assert_eq!(
            encode_key(press(Key::Char('D'), true, false, false)),
            Some(KeyInput::Bytes(vec![0x04]))
        );
        assert_eq!(
            encode_key(press(Key::Char('l'), true, false, false)),
            Some(KeyInput::Bytes(vec![0x0c]))
        );
        assert_eq!(
            encode_key(press(Key::Char('R'), true, false, false)),
            Some(KeyInput::Bytes(vec![0x12]))
        );
        assert_eq!(encode_key(press(Key::Char('e'), true, false, true)), None);
        assert_eq!(encode_key(press(Key::Char('E'), true, false, true)), None);
        assert_eq!(encode_key(press(Key::Char('x'), true, false, false)), None);
        assert_eq!(encode_key(press(Key::Up, true, false, false)), None);
        assert_eq!(encode_key(press(Key::Char('a'), false, true, false)), None);
        assert_eq!(encode_key(press(Key::Tab, false, false, true)), None);
        assert_eq!(encode_key(press(Key::Other, false, false, false)), None);
    }
}
