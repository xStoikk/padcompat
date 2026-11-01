use crate::bits::{
    connection_from_bustype, is_controller_interface, is_event_handler, is_gamepad_node,
    is_joystick_handler,
};
use crate::model::{
    Availability, Connection, Controller, DeviceEvent, DriverBinding, Identity, InputNode,
    ObservedUsb, Snapshot, UsbInterface,
};
use crate::profile::{ControllerProfile, identify};

/// Build a controller snapshot from already-parsed USB and input records.
pub fn assemble(
    usbs: &[ObservedUsb],
    inputs: &[InputNode],
    profiles: &[ControllerProfile],
    kernel_release: &str,
) -> Snapshot {
    let mut used = vec![false; inputs.len()];
    let mut controllers = Vec::new();

    for usb in usbs {
        let mut related = Vec::new();
        for (index, node) in inputs.iter().enumerate() {
            if usb_sysname_in_sysfs(&node.sysfs, &usb.sysname) {
                used[index] = true;
                related.push(node);
            }
        }
        if let Some(controller) = controller_from_usb(usb, &related, profiles) {
            controllers.push(controller);
        }
    }

    let mut groups: Vec<(String, Vec<&InputNode>)> = Vec::new();
    for (index, node) in inputs.iter().enumerate() {
        if used[index] {
            continue;
        }
        let key = input_parent_key(node);
        if let Some((_, group)) = groups.iter_mut().find(|(existing, _)| existing == &key) {
            group.push(node);
        } else {
            groups.push((key, vec![node]));
        }
    }
    for (key, group) in groups {
        if group.iter().any(|node| is_gamepad_node(node)) {
            controllers.push(controller_from_inputs(&key, &group));
        }
    }

    controllers.sort_by(|left, right| {
        (&left.instance, left.vendor_id, left.product_id).cmp(&(
            &right.instance,
            right.vendor_id,
            right.product_id,
        ))
    });

    Snapshot {
        kernel_release: kernel_release.to_string(),
        controllers,
        warnings: Vec::new(),
    }
}

pub fn diff(prev: &Snapshot, next: &Snapshot) -> Vec<DeviceEvent> {
    let mut events = Vec::new();
    for old in &prev.controllers {
        match next
            .controllers
            .iter()
            .find(|controller| controller.instance == old.instance)
        {
            None => events.push(DeviceEvent::Disconnected {
                instance: old.instance.clone(),
                name: old.name.clone(),
                vendor_id: old.vendor_id,
                product_id: old.product_id,
            }),
            Some(updated) if updated != old => events.push(DeviceEvent::Updated(updated.clone())),
            Some(_) => {}
        }
    }
    for controller in &next.controllers {
        if !prev
            .controllers
            .iter()
            .any(|old| old.instance == controller.instance)
        {
            events.push(DeviceEvent::Connected(controller.clone()));
        }
    }
    events
}

pub(crate) fn usb_sysname_in_sysfs(sysfs: &str, sysname: &str) -> bool {
    if sysname.is_empty() {
        return false;
    }
    let path = sysfs.strip_prefix("/sys").unwrap_or(sysfs);
    let iface_prefix = format!("{sysname}:");
    path.split('/')
        .any(|component| component == sysname || component.starts_with(&iface_prefix))
}

fn controller_from_usb(
    usb: &ObservedUsb,
    inputs: &[&InputNode],
    profiles: &[ControllerProfile],
) -> Option<Controller> {
    let identity = identify(usb, profiles);
    let classed = classify_nodes(inputs);
    if !should_report(usb, &classed, &identity) {
        return None;
    }
    let profile = match &identity {
        Identity::Known { profile_id } => profiles.iter().find(|profile| &profile.id == profile_id),
        _ => None,
    };
    let mut interfaces = usb.interfaces.clone();
    interfaces.sort_by(|left, right| {
        left.number
            .cmp(&right.number)
            .then(left.class.cmp(&right.class))
            .then(left.subclass.cmp(&right.subclass))
            .then(left.protocol.cmp(&right.protocol))
            .then(left.driver.cmp(&right.driver))
    });
    Some(Controller {
        instance: usb.sysname.clone(),
        vendor_id: usb.vendor_id,
        product_id: usb.product_id,
        name: device_name(usb, profile, classed.input_name.as_deref()),
        catalog_name: profile.map(|profile| profile.name.clone()),
        input_name: classed.input_name,
        serial: usb.serial.clone(),
        connection: Connection::Usb,
        input_bus: classed.input_bus,
        form: profile.map(|profile| profile.form),
        interfaces,
        driver_binding: driver_binding(profile, &usb.interfaces),
        expected_driver: profile.map(|profile| profile.expected_driver.clone()),
        availability: if classed.gamepad {
            Availability::Available
        } else {
            Availability::NoGamepadInput
        },
        gamepad_evdev: classed.gamepad_evdev,
        joystick: classed.joystick,
        other_evdev: classed.other_evdev,
        identity,
        notes: profile.and_then(|profile| profile.notes.clone()),
    })
}

fn controller_from_inputs(instance_key: &str, inputs: &[&InputNode]) -> Controller {
    let classed = classify_nodes(inputs);
    let primary = inputs
        .iter()
        .copied()
        .find(|node| is_gamepad_node(node))
        .expect("caller checks for a gamepad node");
    let name = if primary.name.is_empty() {
        format!("{:04x}:{:04x}", primary.vendor_id, primary.product_id)
    } else {
        primary.name.clone()
    };
    Controller {
        instance: format!("input:{instance_key}"),
        vendor_id: primary.vendor_id,
        product_id: primary.product_id,
        name,
        catalog_name: None,
        input_name: classed.input_name,
        serial: None,
        connection: connection_from_bustype(primary.bustype),
        input_bus: classed.input_bus,
        form: None,
        interfaces: Vec::new(),
        identity: Identity::Unknown,
        expected_driver: None,
        driver_binding: DriverBinding::NotApplicable,
        availability: Availability::Available,
        gamepad_evdev: classed.gamepad_evdev,
        joystick: classed.joystick,
        other_evdev: classed.other_evdev,
        notes: None,
    }
}

struct ClassifiedNodes {
    input_name: Option<String>,
    input_bus: Connection,
    gamepad_evdev: Vec<String>,
    joystick: Vec<String>,
    other_evdev: Vec<String>,
    gamepad: bool,
}

fn classify_nodes(nodes: &[&InputNode]) -> ClassifiedNodes {
    let primary = nodes
        .iter()
        .copied()
        .find(|node| is_gamepad_node(node) && node.handlers.iter().any(|h| is_joystick_handler(h)))
        .or_else(|| nodes.iter().copied().find(|node| is_gamepad_node(node)));
    let mut gamepad_evdev = Vec::new();
    let mut joystick = Vec::new();
    let mut other_evdev = Vec::new();
    for node in nodes {
        let gamepad = is_gamepad_node(node);
        for handler in &node.handlers {
            if is_event_handler(handler) {
                let path = format!("/dev/input/{handler}");
                if gamepad {
                    gamepad_evdev.push(path);
                } else {
                    other_evdev.push(path);
                }
            } else if is_joystick_handler(handler) {
                joystick.push(format!("/dev/input/{handler}"));
            }
        }
    }
    sort_devnodes(&mut gamepad_evdev);
    sort_devnodes(&mut joystick);
    sort_devnodes(&mut other_evdev);
    ClassifiedNodes {
        input_name: primary
            .map(|node| node.name.clone())
            .filter(|name| !name.is_empty()),
        input_bus: primary
            .map(|node| connection_from_bustype(node.bustype))
            .unwrap_or(Connection::Unknown),
        gamepad: primary.is_some(),
        gamepad_evdev,
        joystick,
        other_evdev,
    }
}

fn should_report(usb: &ObservedUsb, classed: &ClassifiedNodes, identity: &Identity) -> bool {
    if !matches!(identity, Identity::Unknown) {
        return true;
    }
    if classed.gamepad {
        return true;
    }
    usb.interfaces.iter().any(|iface| {
        matches!(
            (iface.class, iface.subclass, iface.protocol),
            (Some(class), Some(subclass), Some(protocol))
                if is_controller_interface(class, subclass, protocol)
        )
    })
}

fn driver_binding(
    profile: Option<&ControllerProfile>,
    interfaces: &[UsbInterface],
) -> DriverBinding {
    let Some(profile) = profile else {
        return DriverBinding::NotApplicable;
    };
    let mut any = false;
    let mut expected = false;
    for iface in interfaces {
        if let Some(driver) = &iface.driver {
            any = true;
            if driver == &profile.expected_driver {
                expected = true;
            }
        }
    }
    if expected {
        DriverBinding::Expected
    } else if any {
        DriverBinding::Missing
    } else {
        DriverBinding::Unbound
    }
}

fn device_name(
    usb: &ObservedUsb,
    profile: Option<&ControllerProfile>,
    input_name: Option<&str>,
) -> String {
    if let Some(product) = usb.product.as_deref().filter(|name| !name.is_empty()) {
        return product.to_string();
    }
    if let Some(profile) = profile {
        return profile.name.clone();
    }
    if let Some(name) = input_name.filter(|name| !name.is_empty()) {
        return name.to_string();
    }
    format!("{:04x}:{:04x}", usb.vendor_id, usb.product_id)
}

fn input_parent_key(node: &InputNode) -> String {
    if node.sysfs.is_empty() {
        return format!("anon:{}:{}", node.name, node.phys);
    }
    const MARK: &str = "/input/input";
    if let Some(index) = node.sysfs.rfind(MARK) {
        let rest = &node.sysfs[index + MARK.len()..];
        if !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()) {
            return node.sysfs[..index].to_string();
        }
    }
    node.sysfs.clone()
}

fn sort_devnodes(paths: &mut Vec<String>) {
    paths.sort_by(|left, right| devnode_key(left).cmp(&devnode_key(right)));
    paths.dedup();
}

fn devnode_key(path: &str) -> (u8, u32, &str) {
    let name = path.rsplit('/').next().unwrap_or(path);
    if let Some(rest) = name.strip_prefix("event") {
        if let Ok(number) = rest.parse::<u32>() {
            return (0, number, name);
        }
    }
    if let Some(rest) = name.strip_prefix("js") {
        if let Ok(number) = rest.parse::<u32>() {
            return (1, number, name);
        }
    }
    (2, 0, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DeviceForm;
    use crate::profile::{InterfaceSelector, builtin_profiles};

    fn iface(
        number: u8,
        class: u8,
        subclass: u8,
        protocol: u8,
        driver: Option<&str>,
    ) -> UsbInterface {
        UsbInterface {
            number: Some(number),
            class: Some(class),
            subclass: Some(subclass),
            protocol: Some(protocol),
            driver: driver.map(str::to_string),
        }
    }

    fn g7_se() -> ObservedUsb {
        ObservedUsb {
            sysname: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            product: Some("GameSir-G7 SE Controller for Xbox".into()),
            serial: Some("TESTSERIAL".into()),
            interfaces: vec![
                iface(0, 0xff, 0x47, 0xd0, Some("xone-wired")),
                iface(1, 0xff, 0x47, 0xd0, Some("xone-wired")),
                iface(2, 0xff, 0x47, 0xd0, None),
            ],
        }
    }

    fn g7_input() -> InputNode {
        InputNode {
            name: "Microsoft Xbox Controller".into(),
            phys: "gip0.0/input0".into(),
            sysfs: "/devices/pci0000:00/0000:00:14.0/usb1/1-6/1-6:1.0/gip0/gip0.0/input/input48"
                .into(),
            bustype: 0x0006,
            vendor_id: 0x3537,
            product_id: 0x1010,
            handlers: vec!["kbd".into(), "event30".into(), "js2".into()],
            key_bits: crate::bits::parse_bitmap("7cdb000000000000 0 8000000000 0 0").unwrap(),
            abs_bits: crate::bits::parse_bitmap("3003f").unwrap(),
        }
    }

    fn razer() -> ObservedUsb {
        ObservedUsb {
            sysname: "1-9".into(),
            vendor_id: 0x1532,
            product_id: 0x0a57,
            product: Some("Razer Wolverine V3 Pro for PC".into()),
            serial: None,
            interfaces: vec![
                iface(0, 0xff, 0x5d, 0x01, Some("xpad-noone")),
                iface(1, 0x03, 0x01, 0x01, Some("usbhid")),
                iface(2, 0x03, 0x00, 0x00, Some("usbhid")),
            ],
        }
    }

    fn razer_pad() -> InputNode {
        InputNode {
            name: "Generic X-Box pad".into(),
            phys: "usb-0000:00:14.0-9/input0".into(),
            sysfs: "/devices/pci0000:00/0000:00:14.0/usb1/1-9/1-9:1.0/input/input25".into(),
            bustype: 0x0003,
            vendor_id: 0x1532,
            product_id: 0x0a57,
            handlers: vec!["event17".into(), "js1".into()],
            key_bits: crate::bits::parse_bitmap("7cdb000000000000 0 0 0 0").unwrap(),
            abs_bits: crate::bits::parse_bitmap("3003f").unwrap(),
        }
    }

    fn razer_keyboard() -> InputNode {
        InputNode {
            name: "Razer Wolverine V3 Pro Keyboard".into(),
            phys: "usb-0000:00:14.0-9/input1".into(),
            sysfs: "/devices/pci0000:00/0000:00:14.0/usb1/1-9/1-9:1.1/input/input19".into(),
            bustype: 0x0003,
            vendor_id: 0x1532,
            product_id: 0x0a57,
            handlers: vec!["event12".into(), "event2".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        }
    }

    #[test]
    fn usb_path_match_is_a_component_match() {
        let gip = "/devices/pci/usb1/1-6/1-6:1.0/gip0/gip0.0/input/input48";
        assert!(usb_sysname_in_sysfs(gip, "1-6"));
        assert!(!usb_sysname_in_sysfs(gip, "1-60"));
        assert!(!usb_sysname_in_sysfs(
            "/devices/usb1/11-6/11-6:1.0/input/input1",
            "1-6"
        ));
        assert!(!usb_sysname_in_sysfs(gip, ""));
    }

    #[test]
    fn gamesir_g7_se_is_one_device_with_duplicate_interfaces() {
        let profiles = builtin_profiles().unwrap();
        let snap = assemble(&[g7_se()], &[g7_input()], &profiles, "7.3.1-1-cachyos");
        assert_eq!(snap.controllers.len(), 1);
        let controller = &snap.controllers[0];
        assert_eq!(controller.interfaces.len(), 3);
        assert_eq!(controller.instance, "1-6");
        assert_eq!(controller.name, "GameSir-G7 SE Controller for Xbox");
        assert_eq!(
            controller.input_name.as_deref(),
            Some("Microsoft Xbox Controller")
        );
        assert_eq!(controller.connection, Connection::Usb);
        assert_eq!(controller.input_bus, Connection::Virtual);
        assert_eq!(controller.form, Some(DeviceForm::Wired));
        assert_eq!(
            controller.identity,
            Identity::Known {
                profile_id: "gamesir-g7-se".into()
            }
        );
        assert_eq!(controller.driver_binding, DriverBinding::Expected);
        assert_eq!(controller.availability, Availability::Available);
        assert_eq!(
            controller.gamepad_evdev,
            vec!["/dev/input/event30".to_string()]
        );
        assert_eq!(controller.joystick, vec!["/dev/input/js2".to_string()]);
        assert!(controller.other_evdev.is_empty());
        assert_eq!(controller.interfaces[2].driver, None);
    }

    #[test]
    fn missing_product_and_driver_fall_back_without_dropping_the_device() {
        let profiles = builtin_profiles().unwrap();
        let mut device = g7_se();
        device.product = None;
        for iface in &mut device.interfaces {
            iface.driver = None;
        }
        let snap = assemble(&[device], &[], &profiles, "k");
        assert_eq!(snap.controllers.len(), 1);
        let controller = &snap.controllers[0];
        assert_eq!(controller.name, "GameSir G7 SE");
        assert_eq!(controller.driver_binding, DriverBinding::Unbound);
        assert_eq!(controller.availability, Availability::NoGamepadInput);
        assert!(controller.input_name.is_none());
    }

    #[test]
    fn unexpected_driver_is_missing_not_unbound() {
        let profiles = builtin_profiles().unwrap();
        let mut device = g7_se();
        for iface in &mut device.interfaces {
            iface.driver = Some("usbhid".into());
        }
        let snap = assemble(&[device], &[g7_input()], &profiles, "k");
        assert_eq!(snap.controllers[0].driver_binding, DriverBinding::Missing);
    }

    #[test]
    fn razer_composite_device_is_one_controller() {
        let profiles = builtin_profiles().unwrap();
        let snap = assemble(&[razer()], &[razer_keyboard(), razer_pad()], &profiles, "k");
        assert_eq!(snap.controllers.len(), 1);
        let controller = &snap.controllers[0];
        assert_eq!(
            controller.identity,
            Identity::Known {
                profile_id: "razer-wolverine-v3-pro-8k".into()
            }
        );
        assert_eq!(controller.driver_binding, DriverBinding::Expected);
        assert_eq!(controller.input_bus, Connection::Usb);
        assert_eq!(controller.form, Some(DeviceForm::Wired));
        assert_eq!(
            controller.catalog_name.as_deref(),
            Some("Razer Wolverine V3 Pro 8K")
        );
        assert_eq!(
            controller.gamepad_evdev,
            vec!["/dev/input/event17".to_string()]
        );
        assert_eq!(controller.joystick, vec!["/dev/input/js1".to_string()]);
        assert_eq!(
            controller.other_evdev,
            vec!["/dev/input/event2".to_string(), "/dev/input/event12".into()]
        );
        assert_eq!(controller.input_name.as_deref(), Some("Generic X-Box pad"));
    }

    #[test]
    fn gamesir_g7_pro_receiver_uses_the_observed_interface() {
        let profiles = builtin_profiles().unwrap();
        let usb = ObservedUsb {
            sysname: "1-7".into(),
            vendor_id: 0x3537,
            product_id: 0x1033,
            product: Some("Xbox 360 Controller for Windows".into()),
            serial: Some("DO-NOT-PRINT".into()),
            interfaces: vec![
                iface(0, 0xff, 0x5d, 0x01, Some("xpad-noone")),
                iface(1, 0x03, 0x00, 0x00, Some("usbhid")),
            ],
        };
        let pad = InputNode {
            name: "Generic X-Box pad".into(),
            phys: "usb-0000:00:14.0-7/input0".into(),
            sysfs: "/devices/pci0000:00/0000:00:14.0/usb1/1-7/1-7:1.0/input/input60".into(),
            bustype: 0x0003,
            vendor_id: 0x3537,
            product_id: 0x1033,
            handlers: vec!["event9".into(), "js0".into()],
            key_bits: crate::bits::parse_bitmap("7cdb000000000000 0 0 0 0").unwrap(),
            abs_bits: crate::bits::parse_bitmap("3003f").unwrap(),
        };
        let consumer = InputNode {
            name: "Xbox 360 Controller for Windows Consumer Control".into(),
            phys: "usb-0000:00:14.0-7/input1".into(),
            sysfs: "/devices/pci0000:00/0000:00:14.0/usb1/1-7/1-7:1.1/input/input61".into(),
            bustype: 0x0003,
            vendor_id: 0x3537,
            product_id: 0x1033,
            handlers: vec!["event10".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let snap = assemble(&[usb], &[pad, consumer], &profiles, "k");
        assert_eq!(snap.controllers.len(), 1);
        let controller = &snap.controllers[0];
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
        assert_eq!(controller.name, "Xbox 360 Controller for Windows");
        assert_eq!(controller.form, Some(DeviceForm::UsbReceiver));
        assert_eq!(controller.driver_binding, DriverBinding::Expected);
        assert_eq!(controller.availability, Availability::Available);
        assert_eq!(controller.connection, Connection::Usb);
        assert_eq!(controller.input_bus, Connection::Usb);
        assert_eq!(
            controller.gamepad_evdev,
            vec!["/dev/input/event9".to_string()]
        );
        assert_eq!(controller.joystick, vec!["/dev/input/js0".to_string()]);
        assert_eq!(
            controller.other_evdev,
            vec!["/dev/input/event10".to_string()]
        );
        assert_eq!(controller.serial.as_deref(), Some("DO-NOT-PRINT"));
    }

    #[test]
    fn unknown_gamepad_is_listed_and_plain_keyboard_is_not() {
        let profiles = builtin_profiles().unwrap();
        let unknown = ObservedUsb {
            sysname: "1-4".into(),
            vendor_id: 0x1234,
            product_id: 0x5678,
            product: None,
            serial: None,
            interfaces: vec![iface(0, 0xff, 0x5d, 0x01, Some("xpad-noone"))],
        };
        let unknown_input = InputNode {
            name: "Generic X-Box pad".into(),
            phys: "usb-x".into(),
            sysfs: "/devices/usb1/1-4/1-4:1.0/input/input3".into(),
            bustype: 0x0003,
            vendor_id: 0x1234,
            product_id: 0x5678,
            handlers: vec!["event4".into(), "js0".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let keyboard = ObservedUsb {
            sysname: "1-2".into(),
            vendor_id: 0x046d,
            product_id: 0xc336,
            product: Some("Logitech Gaming Keyboard G213".into()),
            serial: None,
            interfaces: vec![iface(0, 0x03, 0x01, 0x01, Some("usbhid"))],
        };
        let keyboard_input = InputNode {
            name: "Logitech Gaming Keyboard G213".into(),
            phys: "usb-y".into(),
            sysfs: "/devices/usb1/1-2/1-2:1.0/input/input8".into(),
            bustype: 0x0003,
            vendor_id: 0x046d,
            product_id: 0xc336,
            handlers: vec!["event5".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let hid_named_gamepad = ObservedUsb {
            sysname: "1-7".into(),
            vendor_id: 0x3537,
            product_id: 0x0575,
            product: Some("Gamepad".into()),
            serial: None,
            interfaces: vec![iface(0, 0x03, 0x00, 0x00, Some("usbhid"))],
        };
        let hid_input = InputNode {
            name: "Gamepad".into(),
            phys: "usb-z".into(),
            sysfs: "/devices/usb1/1-7/1-7:1.0/input/input58".into(),
            bustype: 0x0003,
            vendor_id: 0x3537,
            product_id: 0x0575,
            handlers: vec!["event10".into()],
            key_bits: Vec::new(),
            abs_bits: vec![0x0100_0000_0000],
        };
        let snap = assemble(
            &[unknown, keyboard, hid_named_gamepad],
            &[unknown_input, keyboard_input, hid_input],
            &profiles,
            "k",
        );
        assert_eq!(snap.controllers.len(), 1);
        assert_eq!(snap.controllers[0].vendor_id, 0x1234);
        assert_eq!(snap.controllers[0].identity, Identity::Unknown);
        assert_eq!(snap.controllers[0].name, "Generic X-Box pad");
        assert_eq!(
            snap.controllers[0].driver_binding,
            DriverBinding::NotApplicable
        );
    }

    #[test]
    fn controller_interface_without_input_is_still_reported() {
        let profiles = builtin_profiles().unwrap();
        let unknown = ObservedUsb {
            sysname: "2-1".into(),
            vendor_id: 0x9999,
            product_id: 0x0001,
            product: Some("Mystery Pad".into()),
            serial: None,
            interfaces: vec![iface(0, 0xff, 0x47, 0xd0, None)],
        };
        let snap = assemble(&[unknown], &[], &profiles, "k");
        assert_eq!(snap.controllers.len(), 1);
        assert_eq!(
            snap.controllers[0].availability,
            Availability::NoGamepadInput
        );
        assert_eq!(snap.controllers[0].identity, Identity::Unknown);
        assert_eq!(
            snap.controllers[0].driver_binding,
            DriverBinding::NotApplicable
        );
    }

    #[test]
    fn vid_pid_without_interface_does_not_claim_the_profile() {
        let profiles = builtin_profiles().unwrap();
        let device = ObservedUsb {
            sysname: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            product: None,
            serial: None,
            interfaces: vec![UsbInterface {
                number: Some(0),
                class: None,
                subclass: None,
                protocol: None,
                driver: None,
            }],
        };
        let snap = assemble(&[device], &[g7_input()], &profiles, "k");
        assert_eq!(snap.controllers.len(), 1);
        assert_eq!(snap.controllers[0].identity, Identity::Unknown);
        assert_eq!(snap.controllers[0].name, "Microsoft Xbox Controller");
    }

    #[test]
    fn bluetooth_gamepad_without_usb_parent_is_unknown() {
        let node = InputNode {
            name: "Wireless Controller".into(),
            phys: "aa:bb:cc:dd:ee:ff".into(),
            sysfs: "/devices/pci/bluetooth/hci0/input/input9".into(),
            bustype: 0x0005,
            vendor_id: 0x054c,
            product_id: 0x09cc,
            handlers: vec!["event8".into(), "js0".into()],
            key_bits: Vec::new(),
            abs_bits: Vec::new(),
        };
        let snap = assemble(&[], &[node], &builtin_profiles().unwrap(), "k");
        assert_eq!(snap.controllers.len(), 1);
        assert_eq!(snap.controllers[0].connection, Connection::Bluetooth);
        assert_eq!(snap.controllers[0].identity, Identity::Unknown);
        assert!(snap.controllers[0].interfaces.is_empty());
        assert!(snap.controllers[0].instance.starts_with("input:"));
    }

    #[test]
    fn removal_connect_and_driver_change() {
        let profiles = builtin_profiles().unwrap();
        let present = assemble(
            &[g7_se(), razer()],
            &[g7_input(), razer_pad()],
            &profiles,
            "k",
        );
        let only_razer = assemble(&[razer()], &[razer_pad()], &profiles, "other-kernel");
        let events = diff(&present, &only_razer);
        assert_eq!(events.len(), 1);
        match &events[0] {
            DeviceEvent::Disconnected {
                instance,
                name,
                vendor_id,
                product_id,
            } => {
                assert_eq!(instance, "1-6");
                assert_eq!(name, "GameSir-G7 SE Controller for Xbox");
                assert_eq!((*vendor_id, *product_id), (0x3537, 0x1010));
            }
            other => panic!("expected disconnect, got {other:?}"),
        }

        let connected = diff(&only_razer, &present);
        assert!(matches!(connected[0], DeviceEvent::Connected(_)));
        assert!(diff(&present, &present).is_empty());

        let mut changed = present.clone();
        changed.kernel_release = "ignored".into();
        changed.warnings.push("ignored".into());
        assert!(diff(&present, &changed).is_empty());

        changed.controllers[0].driver_binding = DriverBinding::Missing;
        let updates = diff(&present, &changed);
        assert!(matches!(updates[0], DeviceEvent::Updated(_)));
    }

    #[test]
    fn devnode_numbers_sort_numerically() {
        let mut paths = vec!["/dev/input/event10".to_string(), "/dev/input/event2".into()];
        sort_devnodes(&mut paths);
        assert_eq!(
            paths,
            vec!["/dev/input/event2".to_string(), "/dev/input/event10".into()]
        );
    }

    #[test]
    fn ambiguous_identity_does_not_pick_a_driver_expectation() {
        let selector = InterfaceSelector {
            class: 0xff,
            subclass: 0x47,
            protocol: 0xd0,
            number: None,
        };
        let profiles = vec![
            ControllerProfile {
                id: "a".into(),
                name: "A".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: DeviceForm::Wired,
                interfaces: vec![selector.clone()],
                notes: None,
            },
            ControllerProfile {
                id: "b".into(),
                name: "B".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "other".into(),
                form: DeviceForm::Wired,
                interfaces: vec![selector],
                notes: None,
            },
        ];
        let snap = assemble(&[g7_se()], &[g7_input()], &profiles, "k");
        assert_eq!(
            snap.controllers[0].identity,
            Identity::Ambiguous {
                profile_ids: vec!["a".into(), "b".into()]
            }
        );
        assert_eq!(
            snap.controllers[0].driver_binding,
            DriverBinding::NotApplicable
        );
        assert!(snap.controllers[0].form.is_none());
    }
}
