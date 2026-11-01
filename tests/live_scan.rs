use std::fs;
use std::path::Path;

use padcompat::{
    Availability, Connection, DeviceForm, DriverBinding, Identity, ReportOptions, builtin_profiles,
    format_snapshot, format_snapshot_json, scan_system,
};

#[test]
fn live_scan_matches_connected_known_controllers() {
    let profiles = builtin_profiles().expect("builtin profiles");
    let snapshot = scan_system(&profiles).expect("scan");
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap();
    assert_eq!(snapshot.kernel_release, kernel.trim());

    if sysfs_has(0x3537, 0x1010) {
        let found = matching(&snapshot, 0x3537, 0x1010);
        assert_eq!(found.len(), 1, "warnings: {:?}", snapshot.warnings);
        let controller = found[0];
        assert_eq!(
            controller.identity,
            Identity::Known {
                profile_id: "gamesir-g7-se".into()
            }
        );
        assert_eq!(controller.driver_binding, DriverBinding::Expected);
        assert_eq!(controller.availability, Availability::Available);
        assert_eq!(controller.connection, Connection::Usb);
        assert_eq!(controller.input_bus, Connection::Virtual);
        assert_eq!(controller.form, Some(DeviceForm::Wired));
        assert!(controller.name.contains("GameSir-G7 SE"));
        assert_eq!(
            controller.input_name.as_deref(),
            Some("Microsoft Xbox Controller")
        );
        assert!(controller.interfaces.iter().any(|iface| {
            iface.class == Some(0xff)
                && iface.subclass == Some(0x47)
                && iface.protocol == Some(0xd0)
                && iface.driver.as_deref() == Some("xone-wired")
        }));
        assert!(!controller.joystick.is_empty());
        assert!(!controller.gamepad_evdev.is_empty());
    }

    if sysfs_has(0x1532, 0x0a57) {
        let found = matching(&snapshot, 0x1532, 0x0a57);
        assert_eq!(found.len(), 1, "warnings: {:?}", snapshot.warnings);
        let controller = found[0];
        assert_eq!(
            controller.identity,
            Identity::Known {
                profile_id: "razer-wolverine-v3-pro-8k".into()
            }
        );
        assert_eq!(controller.driver_binding, DriverBinding::Expected);
        assert_eq!(controller.availability, Availability::Available);
        assert_eq!(controller.connection, Connection::Usb);
        assert_eq!(controller.input_bus, Connection::Usb);
        assert_eq!(controller.form, Some(DeviceForm::Wired));
        assert_eq!(
            controller.catalog_name.as_deref(),
            Some("Razer Wolverine V3 Pro 8K")
        );
        assert!(controller.name.contains("Wolverine V3 Pro"));
        assert!(controller.interfaces.iter().any(|iface| {
            iface.class == Some(0xff)
                && iface.subclass == Some(0x5d)
                && iface.protocol == Some(0x01)
                && iface.driver.as_deref() == Some("xpad-noone")
        }));
        assert!(!controller.joystick.is_empty());
        assert!(!controller.gamepad_evdev.is_empty());
    }

    if sysfs_has(0x3537, 0x1033) {
        let found = matching(&snapshot, 0x3537, 0x1033);
        assert_eq!(found.len(), 1, "warnings: {:?}", snapshot.warnings);
        let controller = found[0];
        assert_eq!(
            controller.identity,
            Identity::Known {
                profile_id: "gamesir-g7-pro-8k-receiver".into()
            }
        );
        assert_eq!(
            controller.catalog_name.as_deref(),
            Some("GameSir G7 Pro 8K")
        );
        assert_eq!(controller.driver_binding, DriverBinding::Expected);
        assert_eq!(controller.expected_driver.as_deref(), Some("xpad-noone"));
        assert_eq!(controller.availability, Availability::Available);
        assert_eq!(controller.connection, Connection::Usb);
        assert_eq!(controller.input_bus, Connection::Usb);
        assert_eq!(controller.form, Some(DeviceForm::UsbReceiver));
        assert!(controller.interfaces.iter().any(|iface| {
            iface.class == Some(0xff)
                && iface.subclass == Some(0x5d)
                && iface.protocol == Some(0x01)
                && iface.driver.as_deref() == Some("xpad-noone")
        }));
        assert!(!controller.joystick.is_empty());
        assert!(!controller.gamepad_evdev.is_empty());
    }

    if Path::new("/dev/input").is_dir() {
        assert!(
            snapshot
                .warnings
                .iter()
                .all(|warning| !warning.contains("missing devnode")),
            "{:?}",
            snapshot.warnings
        );
    }

    let text = format_snapshot(&snapshot, ReportOptions::default());
    let json = format_snapshot_json(&snapshot, ReportOptions::default()).unwrap();
    for controller in &snapshot.controllers {
        if let Some(serial) = &controller.serial {
            assert!(!text.contains(serial));
            assert!(!json.contains(serial));
        }
    }

    let again = scan_system(&profiles).unwrap();
    assert_eq!(snapshot.controllers, again.controllers);
}

fn matching(
    snapshot: &padcompat::Snapshot,
    vendor: u16,
    product: u16,
) -> Vec<&padcompat::Controller> {
    snapshot
        .controllers
        .iter()
        .filter(|controller| controller.vendor_id == vendor && controller.product_id == product)
        .collect()
}

fn sysfs_has(vendor: u16, product: u16) -> bool {
    let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(vendor_text) = fs::read_to_string(path.join("idVendor")) else {
            continue;
        };
        let Ok(product_text) = fs::read_to_string(path.join("idProduct")) else {
            continue;
        };
        if u16::from_str_radix(vendor_text.trim(), 16).ok() == Some(vendor)
            && u16::from_str_radix(product_text.trim(), 16).ok() == Some(product)
        {
            return true;
        }
    }
    false
}
