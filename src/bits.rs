use crate::model::{Connection, InputNode};

pub(crate) const BTN_JOYSTICK: u32 = 0x120;
pub(crate) const BTN_SOUTH: u32 = 0x130;

/// `words_high_first` is the proc/sysfs bitmap order: the first word holds the
/// highest bits, and each word is a 64-bit kernel `unsigned long`.
pub(crate) fn bitmap_has_bit(words_high_first: &[u64], bit: u32) -> bool {
    const WIDTH: u32 = 64;
    let Some(len) = u32::try_from(words_high_first.len()).ok() else {
        return false;
    };
    if len == 0 {
        return false;
    }
    let from_low = bit / WIDTH;
    if from_low >= len {
        return false;
    }
    let index = (len - 1 - from_low) as usize;
    let shift = bit % WIDTH;
    (words_high_first[index] & (1u64 << shift)) != 0
}

pub(crate) fn parse_bitmap(text: &str) -> Option<Vec<u64>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(Vec::new());
    }
    let mut words = Vec::new();
    for token in text.split_whitespace() {
        if token.len() > 16 {
            return None;
        }
        words.push(u64::from_str_radix(token, 16).ok()?);
    }
    Some(words)
}

pub(crate) fn is_joystick_handler(handler: &str) -> bool {
    prefixed_index(handler, "js")
}

pub(crate) fn is_event_handler(handler: &str) -> bool {
    prefixed_index(handler, "event")
}

fn prefixed_index(handler: &str, prefix: &str) -> bool {
    let Some(rest) = handler.strip_prefix(prefix) else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn is_gamepad_node(node: &InputNode) -> bool {
    node.handlers
        .iter()
        .any(|handler| is_joystick_handler(handler))
        || bitmap_has_bit(&node.key_bits, BTN_SOUTH)
        || bitmap_has_bit(&node.key_bits, BTN_JOYSTICK)
}

/// Xbox-style vendor interfaces used only as a discovery heuristic for devices
/// that have no catalog profile yet. Profile matching does not use this list.
pub(crate) fn is_controller_interface(class: u8, subclass: u8, protocol: u8) -> bool {
    matches!(
        (class, subclass, protocol),
        (0xff, 0x47, 0xd0) | (0xff, 0x5d, 0x01) | (0xff, 0x5d, 0x81)
    )
}

pub(crate) fn connection_from_bustype(bustype: u16) -> Connection {
    match bustype {
        0x0003 => Connection::Usb,
        0x0005 => Connection::Bluetooth,
        0x0006 => Connection::Virtual,
        _ => Connection::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_word_order_treats_the_first_word_as_the_high_bits() {
        let low_set = parse_bitmap("0 1").unwrap();
        assert!(bitmap_has_bit(&low_set, 0));
        assert!(!bitmap_has_bit(&low_set, 64));

        let high_set = parse_bitmap("1 0").unwrap();
        assert!(bitmap_has_bit(&high_set, 64));
        assert!(!bitmap_has_bit(&high_set, 0));
    }

    #[test]
    fn xbox_pad_key_bitmap_has_btn_south_and_abs_sticks() {
        for key in [
            "7cdb000000000000 0 0 0 0",
            "7cdb000000000000 0 8000000000 0 0",
        ] {
            let words = parse_bitmap(key).unwrap();
            assert!(bitmap_has_bit(&words, BTN_SOUTH), "{key}");
            assert!(!bitmap_has_bit(&words, 0), "{key}");
        }
        let abs = parse_bitmap("3003f").unwrap();
        assert!(bitmap_has_bit(&abs, 0x00));
        assert!(bitmap_has_bit(&abs, 0x10));
        assert!(!bitmap_has_bit(&abs, 0x06));
    }

    #[test]
    fn overlong_bitmap_word_is_rejected() {
        assert!(parse_bitmap("10000000000000000").is_none());
        assert_eq!(parse_bitmap("").unwrap(), Vec::<u64>::new());
    }

    #[test]
    fn handler_names_require_a_numeric_suffix() {
        assert!(is_joystick_handler("js0"));
        assert!(is_joystick_handler("js10"));
        assert!(!is_joystick_handler("js"));
        assert!(!is_joystick_handler("jsa"));
        assert!(is_event_handler("event2"));
        assert!(!is_event_handler("event"));
        assert!(!is_event_handler("mice"));
    }
}
