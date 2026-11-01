use std::fmt::Write as _;

use crate::diagnose::{
    DriverComparison, DriverObservation, InputObservation, Recognition, diagnose_controller,
};
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
        let friendly = friendly_name(controller);
        let _ = writeln!(out, "  name: {friendly}");
        if controller.name != friendly {
            let _ = writeln!(out, "  hardware-name: {}", controller.name);
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
        write_diagnostic(&mut out, controller);
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
        DeviceEvent::Connected(controller) => format_presence("connected", controller),
        DeviceEvent::Disconnected {
            instance,
            name,
            vendor_id,
            product_id,
        } => format!(
            "disconnected {instance} {name} {}",
            usb_id(*vendor_id, *product_id)
        ),
        DeviceEvent::Updated(controller) => format_presence("updated", controller),
    }
}

/// Catalog model for a single known match. Unknown and ambiguous devices keep
/// the reported name. This does not change the JSON `name` field.
fn friendly_name(controller: &Controller) -> &str {
    if matches!(controller.identity, Identity::Known { .. }) {
        if let Some(catalog) = controller
            .catalog_name
            .as_deref()
            .filter(|name| !name.is_empty())
        {
            return catalog;
        }
    }
    &controller.name
}

fn format_presence(kind: &str, controller: &Controller) -> String {
    let friendly = friendly_name(controller);
    let mut line = format!(
        "{kind} {} {} {friendly} {} status={} driver={}",
        controller.instance,
        identity_label(&controller.identity),
        usb_id(controller.vendor_id, controller.product_id),
        status_label(controller.availability),
        driver_summary(controller),
    );
    if controller.name != friendly {
        line.push_str(&format!(" hardware-name={}", controller.name));
    }
    line.push_str(&diagnostic_event_suffix(controller));
    line
}

fn write_diagnostic(out: &mut String, controller: &Controller) {
    let diag = diagnose_controller(controller);
    let _ = writeln!(out, "  diagnostic:");
    let _ = writeln!(
        out,
        "    recognition: {}",
        recognition_phrase(&diag.recognition)
    );
    let _ = writeln!(
        out,
        "    driver-observation: {}",
        driver_phrase(&diag.driver)
    );
    if let Some(unbound) = partial_unbound(&diag.driver) {
        let _ = writeln!(out, "    unbound-interfaces: {unbound}");
    }
    if diag.driver.comparison == DriverComparison::ExpectedObserved
        && !diag.driver.additional_drivers.is_empty()
    {
        let _ = writeln!(
            out,
            "    other-drivers: {}",
            diag.driver.additional_drivers.join(",")
        );
    }
    let _ = writeln!(out, "    input: {}", input_phrase(&diag.input));
    let _ = writeln!(out, "    function: not tested");
}

fn diagnostic_event_suffix(controller: &Controller) -> String {
    let diag = diagnose_controller(controller);
    let mut extra = format!(
        " recognition={} driver-observation={} input={} function=not-tested",
        recognition_token(&diag.recognition),
        comparison_token(diag.driver.comparison),
        input_token(&diag.input),
    );
    match diag.driver.comparison {
        DriverComparison::ExpectedObserved => {
            if let Some(unbound) = partial_unbound(&diag.driver) {
                extra.push_str(&format!(" unbound={unbound}"));
            }
            if !diag.driver.additional_drivers.is_empty() {
                extra.push_str(&format!(
                    " other-drivers={}",
                    diag.driver.additional_drivers.join(",")
                ));
            }
        }
        DriverComparison::OtherDriverObserved => {
            if !diag.driver.bound_drivers.is_empty() {
                extra.push_str(&format!(" bound={}", diag.driver.bound_drivers.join(",")));
            }
            if let Some(unbound) = partial_unbound(&diag.driver) {
                extra.push_str(&format!(" unbound={unbound}"));
            }
        }
        DriverComparison::NoDriverObserved => {
            if let Some(name) = diag.driver.expected_driver {
                extra.push_str(&format!(" profile-driver={name}"));
            }
        }
        DriverComparison::NotApplicable => {
            if !diag.driver.bound_drivers.is_empty() {
                extra.push_str(&format!(" bound={}", diag.driver.bound_drivers.join(",")));
            }
        }
    }
    extra
}

fn recognition_phrase(recognition: &Recognition<'_>) -> String {
    match recognition {
        Recognition::Known { profile_id } => format!("known {profile_id}"),
        Recognition::Unknown => "unknown".to_string(),
        Recognition::Ambiguous { profile_ids } => {
            format!("ambiguous {}", profile_ids.join(", "))
        }
    }
}

fn recognition_token(recognition: &Recognition<'_>) -> String {
    match recognition {
        Recognition::Known { profile_id } => format!("known:{profile_id}"),
        Recognition::Unknown => "unknown".to_string(),
        Recognition::Ambiguous { profile_ids } => format!("ambiguous:{}", profile_ids.join(",")),
    }
}

fn comparison_token(comparison: DriverComparison) -> &'static str {
    match comparison {
        DriverComparison::ExpectedObserved => "expected-observed",
        DriverComparison::OtherDriverObserved => "other-driver-observed",
        DriverComparison::NoDriverObserved => "no-driver-observed",
        DriverComparison::NotApplicable => "not-applicable",
    }
}

fn driver_phrase(driver: &DriverObservation<'_>) -> String {
    match driver.comparison {
        DriverComparison::ExpectedObserved => catalog_bound(driver.expected_driver.unwrap_or("?")),
        DriverComparison::OtherDriverObserved => {
            catalog_not_bound(driver.expected_driver.unwrap_or("?"), &driver.bound_drivers)
        }
        DriverComparison::NoDriverObserved => match driver.expected_driver {
            Some(name) => catalog_unbound(name),
            None => "no interface driver bound".to_string(),
        },
        DriverComparison::NotApplicable => {
            if driver.bound_drivers.is_empty() {
                "not applicable".to_string()
            } else {
                format!("not applicable; bound {}", driver.bound_drivers.join(","))
            }
        }
    }
}

fn partial_unbound(driver: &DriverObservation<'_>) -> Option<String> {
    if !matches!(
        driver.comparison,
        DriverComparison::ExpectedObserved | DriverComparison::OtherDriverObserved
    ) {
        return None;
    }
    let labels: Vec<String> = driver
        .interfaces
        .iter()
        .filter(|iface| iface.driver.is_none())
        .map(|iface| match iface.number {
            Some(number) => format!("{number:02x}"),
            None => "??".to_string(),
        })
        .collect();
    if labels.is_empty() {
        None
    } else {
        Some(labels.join(","))
    }
}

fn input_phrase(input: &InputObservation<'_>) -> String {
    let mut parts = Vec::new();
    if input.gamepad_evdev_reported() {
        parts.push("gamepad-evdev registered");
    }
    if input.joystick_reported() {
        parts.push("joystick registered");
    }
    if !input.gamepad_evdev_reported() && !input.joystick_reported() {
        parts.push("no gamepad input registered");
    } else if !input.gamepad_evdev_reported() {
        parts.push("no gamepad evdev");
    }
    if input.other_evdev_reported() {
        parts.push("other-evdev registered");
    }
    parts.join("; ")
}

fn input_token(input: &InputObservation<'_>) -> String {
    let mut parts = Vec::new();
    if input.gamepad_evdev_reported() {
        parts.push("gamepad-evdev");
    }
    if input.joystick_reported() {
        parts.push("joystick");
    }
    if parts.is_empty() {
        parts.push("no-gamepad-input");
    } else if !input.gamepad_evdev_reported() {
        parts.push("no-gamepad-evdev");
    }
    if input.other_evdev_reported() {
        parts.push("other-evdev");
    }
    parts.join(",")
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
    let recorded = controller.expected_driver.as_deref().unwrap_or("?");
    match controller.driver_binding {
        DriverBinding::Expected => catalog_bound(recorded),
        DriverBinding::Missing => catalog_not_bound(recorded, &bound),
        DriverBinding::Unbound => catalog_unbound(recorded),
        DriverBinding::NotApplicable => {
            if bound.is_empty() {
                "n/a".to_string()
            } else {
                bound.join(",")
            }
        }
    }
}

/// Recorded catalog name is among the bound interface drivers.
fn catalog_bound(name: &str) -> String {
    format!("catalog-recorded {name} is bound")
}

/// Recorded catalog name is absent. Bound names are a fact, not a failure.
fn catalog_not_bound(name: &str, bound: &[&str]) -> String {
    if bound.is_empty() {
        format!("catalog-recorded {name} is not bound")
    } else {
        format!(
            "catalog-recorded {name} is not bound; bound {}",
            bound.join(",")
        )
    }
}

/// A profile matched and no interface driver is bound.
fn catalog_unbound(name: &str) -> String {
    format!("no interface driver bound; catalog-recorded {name}")
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
        assert!(text.contains("name: GameSir G7 SE"));
        assert!(text.contains("hardware-name: GameSir-G7 SE Controller for Xbox"));
        assert!(!text.contains("catalog:"));
        assert!(text.contains("driver: catalog-recorded xone-wired is bound"));
        assert!(!text.contains("(expected)"));
        assert!(!text.contains("incompatible"));
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

    #[test]
    fn friendly_name_is_text_only_and_json_name_stays_reported() {
        let profiles = builtin_profiles().unwrap();
        let g7_se = usb(
            "1-6",
            0x3537,
            0x1010,
            Some("GameSir-G7 SE Controller for Xbox"),
            (0xff, 0x47, 0xd0),
            Some("xone-wired"),
        );
        let g7_pro = usb(
            "1-7",
            0x3537,
            0x1033,
            Some("Xbox 360 Controller for Windows"),
            (0xff, 0x5d, 0x01),
            Some("xpad-noone"),
        );
        let wolverine = usb(
            "1-9",
            0x1532,
            0x0a57,
            Some("Razer Wolverine V3 Pro for PC"),
            (0xff, 0x5d, 0x01),
            Some("xpad-noone"),
        );
        let same_as_catalog = usb(
            "1-8",
            0x3537,
            0x1010,
            Some("GameSir G7 SE"),
            (0xff, 0x47, 0xd0),
            Some("xone-wired"),
        );
        let missing_product = usb(
            "1-5",
            0x3537,
            0x1010,
            None,
            (0xff, 0x47, 0xd0),
            Some("xone-wired"),
        );
        let empty_product = usb(
            "1-4",
            0x3537,
            0x1010,
            Some(""),
            (0xff, 0x47, 0xd0),
            Some("xone-wired"),
        );
        let snap = assemble(
            &[
                g7_se,
                g7_pro,
                wolverine,
                same_as_catalog,
                missing_product,
                empty_product,
            ],
            &[],
            &profiles,
            "k",
        );
        let text = format_snapshot(&snap, ReportOptions::default());
        assert!(text.contains("name: GameSir G7 SE"));
        assert!(text.contains("hardware-name: GameSir-G7 SE Controller for Xbox"));
        assert!(text.contains("name: GameSir G7 Pro 8K"));
        assert!(text.contains("hardware-name: Xbox 360 Controller for Windows"));
        assert!(text.contains("name: Razer Wolverine V3 Pro 8K"));
        assert!(text.contains("hardware-name: Razer Wolverine V3 Pro for PC"));
        let same = snap
            .controllers
            .iter()
            .find(|controller| controller.instance == "1-8")
            .unwrap();
        let same_text = format_snapshot(
            &crate::model::Snapshot {
                kernel_release: "k".into(),
                controllers: vec![same.clone()],
                warnings: Vec::new(),
            },
            ReportOptions::default(),
        );
        assert!(same_text.contains("name: GameSir G7 SE"));
        assert!(!same_text.contains("hardware-name:"));
        let missing = snap
            .controllers
            .iter()
            .find(|controller| controller.instance == "1-5")
            .unwrap();
        assert_eq!(missing.name, "GameSir G7 SE");
        assert_eq!(missing.catalog_name.as_deref(), Some("GameSir G7 SE"));
        let missing_text = format_snapshot(&one(missing), ReportOptions::default());
        assert!(missing_text.contains("name: GameSir G7 SE"));
        assert!(!missing_text.contains("hardware-name:"));
        let empty = snap
            .controllers
            .iter()
            .find(|controller| controller.instance == "1-4")
            .unwrap();
        assert_eq!(empty.name, "GameSir G7 SE");
        let empty_text = format_snapshot(&one(empty), ReportOptions::default());
        assert!(empty_text.contains("name: GameSir G7 SE"));
        assert!(!empty_text.contains("hardware-name:"));

        let json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(json.contains("\"name\": \"Xbox 360 Controller for Windows\""));
        assert!(json.contains("\"catalog_name\": \"GameSir G7 Pro 8K\""));
        assert!(json.contains("\"name\": \"GameSir-G7 SE Controller for Xbox\""));
        assert!(!json.contains("hardware-name"));

        let pro = snap
            .controllers
            .iter()
            .find(|controller| controller.instance == "1-7")
            .unwrap();
        let connected = format_event(&DeviceEvent::Connected(pro.clone()));
        assert!(connected.contains("GameSir G7 Pro 8K"));
        assert!(connected.contains("hardware-name=Xbox 360 Controller for Windows"));
        assert!(connected.starts_with("connected 1-7 gamesir-g7-pro-8k-receiver "));
        let updated = format_event(&DeviceEvent::Updated(pro.clone()));
        assert!(updated.starts_with("updated 1-7 "));
        assert!(updated.contains("GameSir G7 Pro 8K"));
        let event_json = format_event_json(
            &DeviceEvent::Connected(pro.clone()),
            ReportOptions::default(),
        )
        .unwrap();
        assert!(event_json.contains("\"name\":\"Xbox 360 Controller for Windows\""));
        assert!(event_json.contains("\"catalog_name\":\"GameSir G7 Pro 8K\""));

        let gone = assemble(&[], &[], &profiles, "k");
        let pro_only = crate::model::Snapshot {
            kernel_release: "k".into(),
            controllers: vec![pro.clone()],
            warnings: Vec::new(),
        };
        let disconnect = format_event(&diff_one(&pro_only, &gone));
        assert_eq!(
            disconnect,
            "disconnected 1-7 Xbox 360 Controller for Windows 3537:1033"
        );
        let disconnect_json =
            format_event_json(&diff_one(&pro_only, &gone), ReportOptions::default()).unwrap();
        assert!(disconnect_json.contains("\"name\":\"Xbox 360 Controller for Windows\""));
        assert!(!disconnect_json.contains("catalog_name"));
    }

    #[test]
    fn unknown_and_ambiguous_reports_do_not_invent_a_model() {
        let unknown_usb = usb(
            "1-3",
            0x1234,
            0x5678,
            Some("Generic X-Box pad"),
            (0xff, 0x5d, 0x01),
            Some("usbhid"),
        );
        let snap = assemble(&[unknown_usb], &[], &builtin_profiles().unwrap(), "k");
        assert_eq!(snap.controllers.len(), 1);
        assert!(matches!(snap.controllers[0].identity, Identity::Unknown));
        let text = format_snapshot(&snap, ReportOptions::default());
        assert!(text.contains("name: Generic X-Box pad"));
        assert!(!text.contains("hardware-name:"));
        assert!(!text.contains("GameSir"));
        let json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(json.contains("\"name\": \"Generic X-Box pad\""));
        assert!(!json.contains("catalog_name"));

        let selector = crate::profile::InterfaceSelector {
            class: 0xff,
            subclass: 0x47,
            protocol: 0xd0,
            number: None,
        };
        let profiles = vec![
            crate::profile::ControllerProfile {
                id: "left".into(),
                name: "Left Model".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: crate::model::DeviceForm::Wired,
                interfaces: vec![selector.clone()],
                notes: None,
            },
            crate::profile::ControllerProfile {
                id: "right".into(),
                name: "Right Model".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: crate::model::DeviceForm::Wired,
                interfaces: vec![selector],
                notes: None,
            },
        ];
        let ambiguous = usb(
            "1-6",
            0x3537,
            0x1010,
            Some("GameSir-G7 SE Controller for Xbox"),
            (0xff, 0x47, 0xd0),
            Some("xone-wired"),
        );
        let snap = assemble(&[ambiguous], &[], &profiles, "k");
        assert!(matches!(
            snap.controllers[0].identity,
            Identity::Ambiguous { .. }
        ));
        assert!(snap.controllers[0].catalog_name.is_none());
        let text = format_snapshot(&snap, ReportOptions::default());
        assert!(text.contains("name: GameSir-G7 SE Controller for Xbox"));
        assert!(!text.contains("name: Left Model"));
        assert!(!text.contains("name: Right Model"));
        assert!(!text.contains("hardware-name:"));
        let connected = format_event(&DeviceEvent::Connected(snap.controllers[0].clone()));
        assert!(connected.contains("ambiguous: left, right"));
        assert!(connected.contains("GameSir-G7 SE Controller for Xbox"));
        assert!(!connected.contains("Left Model"));
        assert!(!connected.contains("Right Model"));
    }

    fn one(controller: &Controller) -> Snapshot {
        Snapshot {
            kernel_release: "k".into(),
            controllers: vec![controller.clone()],
            warnings: Vec::new(),
        }
    }

    #[test]
    fn diagnostic_text_covers_recognition_driver_input_and_events() {
        use crate::model::{Availability, Connection, DeviceForm, DriverBinding};
        use crate::profile::InterfaceSelector;

        let profiles = builtin_profiles().unwrap();
        let g7_se = crate::model::ObservedUsb {
            sysname: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            product: Some("GameSir-G7 SE Controller for Xbox".into()),
            serial: Some("SECRET-SERIAL".into()),
            interfaces: vec![
                crate::model::UsbInterface {
                    number: Some(0),
                    class: Some(0xff),
                    subclass: Some(0x47),
                    protocol: Some(0xd0),
                    driver: Some("xone-wired".into()),
                },
                crate::model::UsbInterface {
                    number: Some(1),
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
        let g7_pro = usb(
            "1-7",
            0x3537,
            0x1033,
            Some("Xbox 360 Controller for Windows"),
            (0xff, 0x5d, 0x01),
            Some("xpad-noone"),
        );
        let wolverine = crate::model::ObservedUsb {
            sysname: "1-9".into(),
            vendor_id: 0x1532,
            product_id: 0x0a57,
            product: Some("Razer Wolverine V3 Pro for PC".into()),
            serial: None,
            interfaces: vec![
                crate::model::UsbInterface {
                    number: Some(0),
                    class: Some(0xff),
                    subclass: Some(0x5d),
                    protocol: Some(0x01),
                    driver: Some("xpad-noone".into()),
                },
                crate::model::UsbInterface {
                    number: Some(1),
                    class: Some(0x03),
                    subclass: Some(0x01),
                    protocol: Some(0x01),
                    driver: Some("usbhid".into()),
                },
                crate::model::UsbInterface {
                    number: Some(2),
                    class: Some(0x03),
                    subclass: Some(0x00),
                    protocol: Some(0x00),
                    driver: Some("usbhid".into()),
                },
            ],
        };
        let input = crate::model::InputNode {
            name: "Microsoft Xbox Controller".into(),
            phys: String::new(),
            sysfs: "/devices/usb1/1-6/1-6:1.0/gip0/input/input1".into(),
            bustype: 0x0006,
            vendor_id: 0x3537,
            product_id: 0x1010,
            handlers: vec!["event30".into(), "js2".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let snap = assemble(&[g7_se, g7_pro, wolverine], &[input], &profiles, "k");
        let text = format_snapshot(&snap, ReportOptions::default());
        assert!(text.contains("name: GameSir G7 SE"));
        assert!(text.contains("hardware-name: GameSir-G7 SE Controller for Xbox"));
        assert!(text.contains("recognition: known gamesir-g7-se"));
        assert!(text.contains("driver-observation: catalog-recorded xone-wired is bound"));
        assert!(text.contains("driver: catalog-recorded xone-wired is bound"));
        assert!(text.contains("unbound-interfaces: 02"));
        assert!(text.contains("input: gamepad-evdev registered; joystick registered"));
        assert!(text.contains("function: not tested"));
        assert!(!text.contains("conflict"));
        assert!(!text.contains("SECRET-SERIAL"));
        assert!(text.contains("name: GameSir G7 Pro 8K"));
        assert!(text.contains("hardware-name: Xbox 360 Controller for Windows"));
        assert!(text.contains("recognition: known gamesir-g7-pro-8k-receiver"));
        assert!(text.contains("driver-observation: catalog-recorded xpad-noone is bound"));
        assert!(text.contains("driver: catalog-recorded xpad-noone is bound"));
        assert!(text.contains("recognition: known razer-wolverine-v3-pro-8k"));
        assert!(text.contains("other-drivers: usbhid"));
        assert!(!text.contains("driver conflict"));

        let json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(json.contains("\"name\": \"Xbox 360 Controller for Windows\""));
        assert!(json.contains("\"catalog_name\": \"GameSir G7 Pro 8K\""));
        assert!(json.contains("\"name\": \"GameSir-G7 SE Controller for Xbox\""));
        assert!(!json.contains("\"diagnostic\""));
        assert!(!json.contains("function_status"));
        assert!(!json.contains("SECRET-SERIAL"));

        let se = snap
            .controllers
            .iter()
            .find(|controller| controller.instance == "1-6")
            .unwrap();
        let connected = format_event(&DeviceEvent::Connected(se.clone()));
        assert!(connected.starts_with("connected 1-6 gamesir-g7-se "));
        assert!(connected.contains("GameSir G7 SE"));
        assert!(connected.contains("hardware-name=GameSir-G7 SE Controller for Xbox"));
        assert!(connected.contains("recognition=known:gamesir-g7-se"));
        assert!(connected.contains("driver=catalog-recorded xone-wired is bound"));
        assert!(connected.contains("driver-observation=expected-observed"));
        assert!(connected.contains("unbound=02"));
        assert!(connected.contains("input=gamepad-evdev,joystick"));
        assert!(connected.contains("function=not-tested"));
        let updated = format_event(&DeviceEvent::Updated(se.clone()));
        assert!(updated.starts_with("updated 1-6 "));
        assert!(updated.contains("driver=catalog-recorded xone-wired is bound"));
        assert!(updated.contains("driver-observation=expected-observed"));
        assert!(updated.contains("recognition=known:gamesir-g7-se"));
        assert!(updated.contains("function=not-tested"));

        let gone = assemble(&[], &[], &profiles, "k");
        let only = one(se);
        let disconnect = format_event(&diff_one(&only, &gone));
        assert_eq!(
            disconnect,
            "disconnected 1-6 GameSir-G7 SE Controller for Xbox 3537:1010"
        );
        assert!(!disconnect.contains("recognition="));
        assert!(!disconnect.contains("function="));

        let unknown = usb(
            "1-3",
            0x1234,
            0x5678,
            Some("Generic X-Box pad"),
            (0xff, 0x5d, 0x01),
            Some("usbhid"),
        );
        let unknown_text = format_snapshot(
            &assemble(&[unknown], &[], &profiles, "k"),
            ReportOptions::default(),
        );
        assert!(unknown_text.contains("name: Generic X-Box pad"));
        assert!(unknown_text.contains("recognition: unknown"));
        assert!(unknown_text.contains("driver: usbhid"));
        assert!(unknown_text.contains("driver-observation: not applicable; bound usbhid"));
        assert!(!unknown_text.contains("catalog-recorded"));
        assert!(!unknown_text.contains("other-drivers:"));
        assert!(!unknown_text.contains("GameSir"));

        let selector = InterfaceSelector {
            class: 0xff,
            subclass: 0x47,
            protocol: 0xd0,
            number: None,
        };
        let ambiguous_profiles = vec![
            crate::profile::ControllerProfile {
                id: "left".into(),
                name: "Left Model".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: DeviceForm::Wired,
                interfaces: vec![selector.clone()],
                notes: None,
            },
            crate::profile::ControllerProfile {
                id: "right".into(),
                name: "Right Model".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: DeviceForm::Wired,
                interfaces: vec![selector],
                notes: None,
            },
        ];
        let ambiguous = usb(
            "1-6",
            0x3537,
            0x1010,
            Some("GameSir-G7 SE Controller for Xbox"),
            (0xff, 0x47, 0xd0),
            Some("xone-wired"),
        );
        let ambiguous_text = format_snapshot(
            &assemble(&[ambiguous], &[], &ambiguous_profiles, "k"),
            ReportOptions::default(),
        );
        assert!(ambiguous_text.contains("recognition: ambiguous left, right"));
        assert!(!ambiguous_text.contains("recognition: known"));
        assert!(!ambiguous_text.contains("name: Left Model"));

        let mut other = se.clone();
        other.interfaces[0].driver = Some("usbhid".into());
        other.interfaces[1].driver = None;
        other.interfaces[2].driver = None;
        other.driver_binding = DriverBinding::Missing;
        other.gamepad_evdev.clear();
        other.joystick = vec!["/dev/input/js0".into()];
        let other_text = format_snapshot(&one(&other), ReportOptions::default());
        assert!(
            other_text.contains("driver: catalog-recorded xone-wired is not bound; bound usbhid")
        );
        assert!(other_text.contains(
            "driver-observation: catalog-recorded xone-wired is not bound; bound usbhid"
        ));
        assert!(other_text.contains("input: joystick registered; no gamepad evdev"));
        assert!(other_text.contains("function: not tested"));
        assert!(!other_text.contains("not observed"));
        assert!(!other_text.contains("incompatible"));
        assert!(!other_text.contains("conflict"));

        let mut none_bound = se.clone();
        for iface in &mut none_bound.interfaces {
            iface.driver = None;
        }
        none_bound.driver_binding = DriverBinding::Unbound;
        none_bound.availability = Availability::NoGamepadInput;
        none_bound.gamepad_evdev.clear();
        none_bound.joystick.clear();
        let none_text = format_snapshot(&one(&none_bound), ReportOptions::default());
        assert!(
            none_text.contains("driver: no interface driver bound; catalog-recorded xone-wired")
        );
        assert!(none_text.contains(
            "driver-observation: no interface driver bound; catalog-recorded xone-wired"
        ));
        assert!(!none_text.contains("expected"));
        assert!(!none_text.contains("unbound-interfaces:"));
        assert!(none_text.contains("input: no gamepad input registered"));

        let mut known_no_usb = se.clone();
        known_no_usb.interfaces.clear();
        known_no_usb.driver_binding = DriverBinding::NotApplicable;
        let known_no_usb_text = format_snapshot(&one(&known_no_usb), ReportOptions::default());
        assert!(known_no_usb_text.contains("recognition: known gamesir-g7-se"));
        assert!(known_no_usb_text.contains("driver-observation: not applicable"));
        assert!(!known_no_usb_text.contains("unbound-interfaces:"));
        assert!(!known_no_usb_text.contains("no interface driver bound"));

        let mut no_usb = se.clone();
        no_usb.interfaces.clear();
        no_usb.expected_driver = None;
        no_usb.driver_binding = DriverBinding::NotApplicable;
        no_usb.identity = Identity::Unknown;
        no_usb.catalog_name = None;
        no_usb.form = None;
        no_usb.connection = Connection::Bluetooth;
        no_usb.input_bus = Connection::Bluetooth;
        no_usb.name = "Wireless Controller".into();
        let bt_text = format_snapshot(&one(&no_usb), ReportOptions::default());
        assert!(bt_text.contains("recognition: unknown"));
        assert!(bt_text.contains("connection: bluetooth"));
        assert!(bt_text.contains("input-bus: bluetooth"));
        assert!(!bt_text.contains("form:"));
        assert!(bt_text.contains("driver-observation: not applicable"));
        assert!(!bt_text.contains("unbound-interfaces:"));
        assert!(bt_text.contains("function: not tested"));
        let bt_event = format_event(&DeviceEvent::Connected(no_usb));
        assert!(bt_event.contains("driver-observation=not-applicable"));
        assert!(bt_event.contains("function=not-tested"));
        assert!(!bt_event.contains("conflict"));
    }

    #[test]
    fn stock_xpad_on_a_catalog_xpad_noone_profile_is_not_called_incompatible() {
        let profiles = builtin_profiles().unwrap();
        let wolverine = crate::model::ObservedUsb {
            sysname: "1-9".into(),
            vendor_id: 0x1532,
            product_id: 0x0a57,
            product: Some("Razer Wolverine V3 Pro for PC".into()),
            serial: Some("DO-NOT-PRINT".into()),
            interfaces: vec![
                crate::model::UsbInterface {
                    number: Some(0),
                    class: Some(0xff),
                    subclass: Some(0x5d),
                    protocol: Some(0x01),
                    driver: Some("xpad".into()),
                },
                crate::model::UsbInterface {
                    number: Some(1),
                    class: Some(0x03),
                    subclass: Some(0x01),
                    protocol: Some(0x01),
                    driver: Some("usbhid".into()),
                },
                crate::model::UsbInterface {
                    number: Some(2),
                    class: Some(0x03),
                    subclass: Some(0x00),
                    protocol: Some(0x00),
                    driver: Some("usbhid".into()),
                },
            ],
        };
        let input = crate::model::InputNode {
            name: "Razer Wolverine V3 Pro (Wired)".into(),
            phys: String::new(),
            sysfs: "/devices/usb1/1-9/1-9:1.0/input/input9".into(),
            bustype: 0x0003,
            vendor_id: 0x1532,
            product_id: 0x0a57,
            handlers: vec!["event11".into(), "js0".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let snap = assemble(&[wolverine], &[input], &profiles, "7.3.1-1-cachyos");
        let controller = &snap.controllers[0];
        assert_eq!(controller.driver_binding, DriverBinding::Missing);
        assert_eq!(controller.expected_driver.as_deref(), Some("xpad-noone"));
        let text = format_snapshot(&snap, ReportOptions::default());
        assert!(text.contains("name: Razer Wolverine V3 Pro 8K"));
        assert!(text.contains("hardware-name: Razer Wolverine V3 Pro for PC"));
        assert!(
            text.contains("driver: catalog-recorded xpad-noone is not bound; bound xpad,usbhid")
        );
        assert!(text.contains(
            "driver-observation: catalog-recorded xpad-noone is not bound; bound xpad,usbhid"
        ));
        assert!(text.contains("    00 ff/5d/01 driver=xpad"));
        assert!(text.contains("    01 03/01/01 driver=usbhid"));
        assert!(text.contains("    02 03/00/00 driver=usbhid"));
        assert!(text.contains("input: gamepad-evdev registered; joystick registered"));
        assert!(text.contains("function: not tested"));
        assert!(!text.contains("other-drivers:"));
        assert!(!text.contains("not observed"));
        assert!(!text.contains("(expected)"));
        assert!(!text.contains("incompatible"));
        assert!(!text.contains("conflict"));
        assert!(!text.contains("DO-NOT-PRINT"));

        let json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(json.contains("\"expected_driver\": \"xpad-noone\""));
        assert!(json.contains("\"driver_binding\": \"missing\""));
        assert!(json.contains("\"driver\": \"xpad\""));
        assert!(json.contains("\"driver\": \"usbhid\""));
        assert!(!json.contains("DO-NOT-PRINT"));
        assert!(!json.contains("catalog-recorded"));
        assert!(!json.contains("diagnostic"));

        let connected = format_event(&DeviceEvent::Connected(controller.clone()));
        assert!(connected.starts_with("connected 1-9 razer-wolverine-v3-pro-8k "));
        assert!(
            connected
                .contains("driver=catalog-recorded xpad-noone is not bound; bound xpad,usbhid")
        );
        assert!(connected.contains("driver-observation=other-driver-observed"));
        assert!(connected.contains("bound=xpad,usbhid"));
        assert!(connected.contains("function=not-tested"));
        assert!(!connected.contains("unbound="));
        let updated = format_event(&DeviceEvent::Updated(controller.clone()));
        assert!(updated.starts_with("updated 1-9 "));
        assert!(
            updated.contains("driver=catalog-recorded xpad-noone is not bound; bound xpad,usbhid")
        );
        assert!(updated.contains("driver-observation=other-driver-observed"));
        assert!(updated.contains("function=not-tested"));

        let gone = assemble(&[], &[], &profiles, "7.3.1-1-cachyos");
        let disconnect = format_event(&diff_one(&snap, &gone));
        assert_eq!(
            disconnect,
            "disconnected 1-9 Razer Wolverine V3 Pro for PC 1532:0a57"
        );
    }

    fn usb(
        sysname: &str,
        vendor: u16,
        product_id: u16,
        product: Option<&str>,
        iface: (u8, u8, u8),
        driver: Option<&str>,
    ) -> crate::model::ObservedUsb {
        let (class, subclass, protocol) = iface;
        crate::model::ObservedUsb {
            sysname: sysname.into(),
            vendor_id: vendor,
            product_id,
            product: product.map(str::to_string),
            serial: None,
            interfaces: vec![crate::model::UsbInterface {
                number: Some(0),
                class: Some(class),
                subclass: Some(subclass),
                protocol: Some(protocol),
                driver: driver.map(str::to_string),
            }],
        }
    }

    fn diff_one(prev: &Snapshot, next: &Snapshot) -> DeviceEvent {
        crate::assemble::diff(prev, next).remove(0)
    }
}
