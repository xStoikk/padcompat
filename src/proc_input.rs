use crate::bits::parse_bitmap;
use crate::model::InputNode;

/// Parse `/proc/bus/input/devices`. Malformed bitmap words become empty bit
/// sets; the node is still returned so a `js*` handler can identify it.
pub fn parse_proc_input(text: &str) -> Vec<InputNode> {
    text.split("\n\n")
        .filter_map(|block| parse_block(block.trim()))
        .collect()
}

fn parse_block(block: &str) -> Option<InputNode> {
    if block.is_empty() {
        return None;
    }
    let mut name = String::new();
    let mut phys = String::new();
    let mut sysfs = String::new();
    let mut bustype = 0u16;
    let mut vendor_id = 0u16;
    let mut product_id = 0u16;
    let mut handlers = Vec::new();
    let mut key_bits = Vec::new();
    let mut abs_bits = Vec::new();
    let mut saw_field = false;

    for line in block.lines() {
        let line = line.trim().trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("I:") {
            saw_field = true;
            if let Some((bus, vendor, product)) = parse_id_line(rest.trim()) {
                bustype = bus;
                vendor_id = vendor;
                product_id = product;
            }
        } else if let Some(rest) = line.strip_prefix("N:") {
            saw_field = true;
            name = unquote(value_after(rest.trim(), "Name="));
        } else if let Some(rest) = line.strip_prefix("P:") {
            saw_field = true;
            phys = value_after(rest.trim(), "Phys=").to_string();
        } else if let Some(rest) = line.strip_prefix("S:") {
            saw_field = true;
            sysfs = value_after(rest.trim(), "Sysfs=").to_string();
        } else if let Some(rest) = line.strip_prefix("H:") {
            saw_field = true;
            handlers = value_after(rest.trim(), "Handlers=")
                .split_whitespace()
                .map(str::to_string)
                .collect();
        } else if let Some(rest) = line.strip_prefix("B:") {
            saw_field = true;
            let rest = rest.trim();
            if let Some(bits) = rest.strip_prefix("KEY=") {
                key_bits = parse_bitmap(bits).unwrap_or_default();
            } else if let Some(bits) = rest.strip_prefix("ABS=") {
                abs_bits = parse_bitmap(bits).unwrap_or_default();
            }
        }
    }

    if !saw_field {
        return None;
    }
    Some(InputNode {
        name,
        phys,
        sysfs,
        bustype,
        vendor_id,
        product_id,
        handlers,
        key_bits,
        abs_bits,
    })
}

fn value_after<'a>(text: &'a str, key: &str) -> &'a str {
    text.strip_prefix(key).unwrap_or(text).trim()
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text[1..text.len() - 1].to_string()
    } else {
        text.to_string()
    }
}

fn parse_id_line(text: &str) -> Option<(u16, u16, u16)> {
    let mut bus = None;
    let mut vendor = None;
    let mut product = None;
    for token in text.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        let value = u16::from_str_radix(value, 16).ok()?;
        match key {
            "Bus" => bus = Some(value),
            "Vendor" => vendor = Some(value),
            "Product" => product = Some(value),
            _ => {}
        }
    }
    Some((bus?, vendor?, product?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
I: Bus=0006 Vendor=3537 Product=1010 Version=0102
N: Name=\"Microsoft Xbox Controller\"
P: Phys=gip0.0/input0
S: Sysfs=/devices/pci0000:00/0000:00:14.0/usb1/1-6/1-6:1.0/gip0/gip0.0/input/input48
H: Handlers=kbd event30 js2
B: KEY=7cdb000000000000 0 8000000000 0 0
B: ABS=3003f

I: Bus=0003 Vendor=1532 Product=0a57 Version=0100
N: Name=\"Razer Razer Wolverine V3 Pro for PC Keyboard\"
P: Phys=usb-0000:00:14.0-9/input1
S: Sysfs=/devices/pci0000:00/0000:00:14.0/usb1/1-9/1-9:1.1/0003:1532:0A57.0006/input/input19
H: Handlers=sysrq kbd event12
B: KEY=zzzz
";

    #[test]
    fn parses_records_and_keeps_a_node_when_the_bitmap_is_garbage() {
        let nodes = parse_proc_input(SAMPLE);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "Microsoft Xbox Controller");
        assert_eq!(nodes[0].bustype, 0x0006);
        assert_eq!(nodes[0].vendor_id, 0x3537);
        assert_eq!(nodes[0].product_id, 0x1010);
        assert_eq!(
            nodes[0].handlers,
            vec!["kbd".to_string(), "event30".into(), "js2".into()]
        );
        assert!(!nodes[0].key_bits.is_empty());
        assert!(nodes[0].sysfs.contains("/1-6/1-6:1.0/"));
        assert_eq!(nodes[1].key_bits, Vec::<u64>::new());
        assert_eq!(
            nodes[1].handlers,
            vec!["sysrq".to_string(), "kbd".into(), "event12".into()]
        );
    }

    #[test]
    fn missing_id_line_still_yields_a_node() {
        let nodes = parse_proc_input("N: Name=\"Pad\"\nH: Handlers=event3\n");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "Pad");
        assert_eq!(nodes[0].vendor_id, 0);
        assert_eq!(nodes[0].bustype, 0);
        assert_eq!(nodes[0].handlers, vec!["event3".to_string()]);
    }
}
