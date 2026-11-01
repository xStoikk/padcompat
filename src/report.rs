use std::fmt::Write as _;

use crate::model::{
    Availability, Controller, DeviceEvent, DriverBinding, Identity, Snapshot, UsbInterface,
};

/// Controls fields that are unsafe to paste into a public report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReportOptions {
    pub show_serial: bool,
}

pub fn format_snapshot(snapshot: &Snapshot, options: ReportOptions) -> String {
    let snapshot = redact_snapshot(snapshot, options);
    let mut out = String::new();
    let _ = writeln!(out, "kernel: {}", snapshot.kernel_release);
    let _ = writeln!(out, "controllers: {}", snapshot.controllers.len());
    if snapshot.controllers.is_empty() {
        let _ = writeln!(out, "no controller-like usb or input devices");
    }
    for controller in &snapshot.controllers {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", identity_label(&controller.identity));
        let _ = writeln!(out, "  name: {}", controller.name);
        if let Some(catalog) = &controller.catalog_name {
            let _ = writeln!(out, "  catalog: {catalog}");
        }
        let _ = writeln!(
            out,
            "  usb: {:04x}:{:04x}",
            controller.vendor_id, controller.product_id
        );
        let _ = writeln!(out, "  instance: {}", controller.instance);
        let _ = writeln!(out, "  connection: {}", controller.connection.as_str());
        if let Some(form) = controller.form {
            let _ = writeln!(out, "  form: {}", form.as_str());
        }
        let _ = writeln!(out, "  input-bus: {}", controller.input_bus.as_str());
        let _ = writeln!(out, "  status: {}", status_label(controller.availability));
        let _ = writeln!(out, "  driver: {}", driver_summary(controller));
        if let Some(serial) = &controller.serial {
            let _ = writeln!(out, "  serial: {serial}");
        }
        let _ = writeln!(out, "  interfaces:");
        if controller.interfaces.is_empty() {
            let _ = writeln!(out, "    none");
        } else {
            for iface in &controller.interfaces {
                let _ = writeln!(out, "{}", format_interface(iface));
            }
        }
        let _ = writeln!(
            out,
            "  gamepad-evdev: {}",
            format_list(&controller.gamepad_evdev)
        );
        let _ = writeln!(out, "  joystick: {}", format_list(&controller.joystick));
        let _ = writeln!(
            out,
            "  other-evdev: {}",
            format_list(&controller.other_evdev)
        );
        if let Some(input_name) = &controller.input_name {
            let _ = writeln!(out, "  input-name: {input_name}");
        }
        if let Some(notes) = &controller.notes {
            let _ = writeln!(out, "  notes: {notes}");
        }
    }
    if !snapshot.warnings.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "warnings:");
        for warning in &snapshot.warnings {
            let _ = writeln!(out, "  - {warning}");
        }
    }
    out
}

pub fn format_snapshot_json(
    snapshot: &Snapshot,
    options: ReportOptions,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&redact_snapshot(snapshot, options))
}

pub fn format_event_json(
    event: &DeviceEvent,
    options: ReportOptions,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&redact_event(event, options))
}

fn redact_snapshot(snapshot: &Snapshot, options: ReportOptions) -> Snapshot {
    if options.show_serial {
        return snapshot.clone();
    }
    let mut snapshot = snapshot.clone();
    for controller in &mut snapshot.controllers {
        controller.serial = None;
    }
    snapshot
}

fn redact_event(event: &DeviceEvent, options: ReportOptions) -> DeviceEvent {
    if options.show_serial {
        return event.clone();
    }
    match event {
        DeviceEvent::Connected(controller) => DeviceEvent::Connected(without_serial(controller)),
        DeviceEvent::Updated(controller) => DeviceEvent::Updated(without_serial(controller)),
        DeviceEvent::Disconnected { .. } => event.clone(),
    }
}

fn without_serial(controller: &Controller) -> Controller {
    let mut controller = controller.clone();
    controller.serial = None;
    controller
}

pub fn format_event(event: &DeviceEvent) -> String {
    match event {
        DeviceEvent::Connected(controller) => format!(
            "connected {} {} {} {} status={} driver={}",
            controller.instance,
            identity_label(&controller.identity),
            controller.name,
            usb_id(controller.vendor_id, controller.product_id),
            status_label(controller.availability),
            driver_summary(controller),
        ),
        DeviceEvent::Disconnected {
            instance,
            name,
            vendor_id,
            product_id,
        } => format!(
            "disconnected {instance} {name} {}",
            usb_id(*vendor_id, *product_id)
        ),
        DeviceEvent::Updated(controller) => format!(
            "updated {} {} {} {} status={} driver={}",
            controller.instance,
            identity_label(&controller.identity),
            controller.name,
            usb_id(controller.vendor_id, controller.product_id),
            status_label(controller.availability),
            driver_summary(controller),
        ),
    }
}

fn identity_label(identity: &Identity) -> String {
    match identity {
        Identity::Known { profile_id } => profile_id.clone(),
        Identity::Unknown => "unknown".to_string(),
        Identity::Ambiguous { profile_ids } => format!("ambiguous: {}", profile_ids.join(", ")),
    }
}

fn status_label(availability: Availability) -> &'static str {
    match availability {
        Availability::Available => "available",
        Availability::NoGamepadInput => "no-gamepad-input",
    }
}

fn driver_summary(controller: &Controller) -> String {
    let bound = unique_drivers(&controller.interfaces);
    match controller.driver_binding {
        DriverBinding::Expected => {
            let expected = controller.expected_driver.as_deref().unwrap_or("?");
            format!("{expected} (expected)")
        }
        DriverBinding::Missing => {
            let expected = controller.expected_driver.as_deref().unwrap_or("?");
            if bound.is_empty() {
                format!("missing {expected}")
            } else {
                format!("expected {expected}, bound {}", bound.join(","))
            }
        }
        DriverBinding::Unbound => {
            let expected = controller.expected_driver.as_deref().unwrap_or("?");
            format!("unbound (expected {expected})")
        }
        DriverBinding::NotApplicable => {
            if bound.is_empty() {
                "n/a".to_string()
            } else {
                bound.join(",")
            }
        }
    }
}

fn unique_drivers(interfaces: &[UsbInterface]) -> Vec<&str> {
    let mut drivers = Vec::new();
    for iface in interfaces {
        if let Some(driver) = iface.driver.as_deref() {
            if !drivers.contains(&driver) {
                drivers.push(driver);
            }
        }
    }
    drivers
}

fn format_interface(iface: &UsbInterface) -> String {
    let number = iface
        .number
        .map(|number| format!("{number:02x}"))
        .unwrap_or_else(|| "??".to_string());
    let driver = iface.driver.as_deref().unwrap_or("unbound");
    format!(
        "    {number} {}/{}/{} driver={driver}",
        hex8(iface.class),
        hex8(iface.subclass),
        hex8(iface.protocol),
    )
}

fn hex8(value: Option<u8>) -> String {
    match value {
        Some(value) => format!("{value:02x}"),
        None => "??".to_string(),
    }
}

fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(" ")
    }
}

fn usb_id(vendor: u16, product: u16) -> String {
    format!("{vendor:04x}:{product:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assemble::assemble;
    use crate::model::DeviceEvent;
    use crate::profile::builtin_profiles;

    #[test]
    fn text_report_includes_unbound_interface_and_expected_driver() {
        let usb = crate::model::ObservedUsb {
            sysname: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            product: Some("GameSir-G7 SE Controller for Xbox".into()),
            serial: None,
            interfaces: vec![
                crate::model::UsbInterface {
                    number: Some(0),
                    class: Some(0xff),
                    subclass: Some(0x47),
                    protocol: Some(0xd0),
                    driver: Some("xone-wired".into()),
                },
                crate::model::UsbInterface {
                    number: Some(2),
                    class: Some(0xff),
                    subclass: Some(0x47),
                    protocol: Some(0xd0),
                    driver: None,
                },
            ],
        };
        let snap = assemble(&[usb], &[], &builtin_profiles().unwrap(), "7.3.1-1-cachyos");
        let text = format_snapshot(&snap, ReportOptions::default());
        assert!(text.contains("kernel: 7.3.1-1-cachyos"));
        assert!(text.contains("gamesir-g7-se"));
        assert!(text.contains("catalog: GameSir G7 SE"));
        assert!(text.contains("driver: xone-wired (expected)"));
        assert!(text.contains("02 ff/47/d0 driver=unbound"));
        assert!(text.contains("status: no-gamepad-input"));
        let json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(json.contains("\"profile_id\": \"gamesir-g7-se\""));

        let gone = assemble(&[], &[], &builtin_profiles().unwrap(), "7.3.1-1-cachyos");
        let event = &diff_one(&snap, &gone);
        let line = format_event(event);
        assert!(line.starts_with("disconnected 1-6 "));
        assert!(line.contains("3537:1010"));
    }

    #[test]
    fn serial_is_hidden_unless_explicitly_requested() {
        let usb = crate::model::ObservedUsb {
            sysname: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            product: Some("GameSir-G7 SE Controller for Xbox".into()),
            serial: Some("SECRET-SERIAL".into()),
            interfaces: vec![crate::model::UsbInterface {
                number: Some(0),
                class: Some(0xff),
                subclass: Some(0x47),
                protocol: Some(0xd0),
                driver: Some("xone-wired".into()),
            }],
        };
        let input = crate::model::InputNode {
            name: "Microsoft Xbox Controller".into(),
            phys: "gip0.0/input0".into(),
            sysfs: "/devices/usb1/1-6/1-6:1.0/gip0/input/input1".into(),
            bustype: 0x0006,
            vendor_id: 0x3537,
            product_id: 0x1010,
            handlers: vec!["event1".into(), "js0".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let snap = assemble(&[usb], &[input], &builtin_profiles().unwrap(), "k");
        assert_eq!(snap.controllers[0].serial.as_deref(), Some("SECRET-SERIAL"));

        let hidden = format_snapshot(&snap, ReportOptions::default());
        let hidden_json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(!hidden.contains("SECRET-SERIAL"));
        assert!(!hidden.contains("serial:"));
        assert!(!hidden_json.contains("SECRET-SERIAL"));
        assert!(!hidden_json.contains("\"serial\""));

        let shown = format_snapshot(&snap, ReportOptions { show_serial: true });
        let shown_json = format_snapshot_json(&snap, ReportOptions { show_serial: true }).unwrap();
        assert!(shown.contains("serial: SECRET-SERIAL"));
        assert!(shown_json.contains("SECRET-SERIAL"));

        let event = DeviceEvent::Connected(snap.controllers[0].clone());
        let event_json = format_event_json(&event, ReportOptions::default()).unwrap();
        assert!(!event_json.contains("SECRET-SERIAL"));
        let event_shown = format_event_json(&event, ReportOptions { show_serial: true }).unwrap();
        assert!(event_shown.contains("SECRET-SERIAL"));
    }

    fn diff_one(prev: &Snapshot, next: &Snapshot) -> DeviceEvent {
        crate::assemble::diff(prev, next).remove(0)
    }
}
