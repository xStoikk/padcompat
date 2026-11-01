//! Pure read of a [`Controller`](crate::Controller).
//!
//! The function does not perform I/O, open evdev nodes, or decide that a
//! controller is broken. Snapshot warnings are not read here: matching them
//! by instance prefix would attach a `1-6` warning to `1-6.1`.

use crate::model::{Availability, Connection, Controller, DeviceForm, DriverBinding, Identity};

/// Catalog match, copied from [`Identity`] without choosing among candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recognition<'a> {
    Known { profile_id: &'a str },
    Unknown,
    Ambiguous { profile_ids: &'a [String] },
}

/// Comparison of `expected_driver` with the drivers actually bound.
///
/// This is not a statement that the pad works or that a package should be
/// installed. [`DriverComparison::NotApplicable`] means there was nothing to
/// compare, including a controller with no USB interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverComparison {
    NotApplicable,
    ExpectedObserved,
    OtherDriverObserved,
    NoDriverObserved,
}

/// One interface, in the controller's existing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceObservation<'a> {
    pub number: Option<u8>,
    pub driver: Option<&'a str>,
}

/// Driver symlinks already stored on the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverObservation<'a> {
    /// Value already computed by discovery. Not recomputed.
    pub binding: DriverBinding,
    pub comparison: DriverComparison,
    pub expected_driver: Option<&'a str>,
    pub interfaces: Vec<InterfaceObservation<'a>>,
    /// First-seen order, each name once.
    pub bound_drivers: Vec<&'a str>,
    /// Bound names other than `expected_driver`, first-seen order.
    /// Empty when there is no expected driver to compare against.
    pub additional_drivers: Vec<&'a str>,
    pub missing_number_count: usize,
    pub duplicate_numbers: Vec<u8>,
}

/// Evdev and joystick paths as discovery reported them. Nodes were not opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputObservation<'a> {
    pub availability: Availability,
    pub gamepad_evdev: &'a [String],
    pub joystick: &'a [String],
    pub other_evdev: &'a [String],
}

impl InputObservation<'_> {
    pub fn gamepad_evdev_reported(self) -> bool {
        !self.gamepad_evdev.is_empty()
    }

    pub fn joystick_reported(self) -> bool {
        !self.joystick.is_empty()
    }

    pub fn other_evdev_reported(self) -> bool {
        !self.other_evdev.is_empty()
    }
}

/// `connection`, `input_bus`, and `form` describe different facts.
/// A USB parent with a virtual input bus is not treated as a disagreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionObservation {
    pub connection: Connection,
    pub input_bus: Connection,
    pub form: Option<DeviceForm>,
}

/// Padcompat does not press buttons or move axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionStatus {
    NotTested,
}

/// A fact a later report may mention. None of these is a malfunction score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    UnknownIdentity,
    AmbiguousIdentity,
    CatalogNameEmpty,
    NoUsbInterfaces,
    ExpectedDriverNotObserved,
    UnboundInterface,
    AdditionalDriver,
    InterfaceNumberMissing,
    InterfaceNumberDuplicated,
    NoGamepadInputRegistered,
}

/// Borrowed view of one controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerDiagnostic<'a> {
    pub recognition: Recognition<'a>,
    /// Set only for [`Recognition::Known`] with a non-empty catalog name.
    pub catalog_name: Option<&'a str>,
    pub driver: DriverObservation<'a>,
    pub input: InputObservation<'a>,
    pub connection: ConnectionObservation,
    pub function: FunctionStatus,
}

impl ControllerDiagnostic<'_> {
    pub fn observations(&self) -> Vec<Observation> {
        let mut notes = Vec::new();
        match self.recognition {
            Recognition::Unknown => notes.push(Observation::UnknownIdentity),
            Recognition::Ambiguous { .. } => notes.push(Observation::AmbiguousIdentity),
            Recognition::Known { .. } if self.catalog_name.is_none() => {
                notes.push(Observation::CatalogNameEmpty);
            }
            Recognition::Known { .. } => {}
        }
        if self.driver.interfaces.is_empty() {
            notes.push(Observation::NoUsbInterfaces);
        }
        if matches!(
            self.driver.comparison,
            DriverComparison::OtherDriverObserved | DriverComparison::NoDriverObserved
        ) {
            notes.push(Observation::ExpectedDriverNotObserved);
        }
        if self
            .driver
            .interfaces
            .iter()
            .any(|iface| iface.driver.is_none())
        {
            notes.push(Observation::UnboundInterface);
        }
        if !self.driver.additional_drivers.is_empty() {
            notes.push(Observation::AdditionalDriver);
        }
        if self.driver.missing_number_count > 0 {
            notes.push(Observation::InterfaceNumberMissing);
        }
        if !self.driver.duplicate_numbers.is_empty() {
            notes.push(Observation::InterfaceNumberDuplicated);
        }
        if self.input.availability == Availability::NoGamepadInput {
            notes.push(Observation::NoGamepadInputRegistered);
        }
        notes
    }
}

pub fn diagnose_controller(controller: &Controller) -> ControllerDiagnostic<'_> {
    let recognition = match &controller.identity {
        Identity::Known { profile_id } => Recognition::Known {
            profile_id: profile_id.as_str(),
        },
        Identity::Unknown => Recognition::Unknown,
        Identity::Ambiguous { profile_ids } => Recognition::Ambiguous { profile_ids },
    };
    let catalog_name = match recognition {
        Recognition::Known { .. } => controller
            .catalog_name
            .as_deref()
            .filter(|name| !name.is_empty()),
        _ => None,
    };
    ControllerDiagnostic {
        recognition,
        catalog_name,
        driver: driver_observation(controller),
        input: InputObservation {
            availability: controller.availability,
            gamepad_evdev: &controller.gamepad_evdev,
            joystick: &controller.joystick,
            other_evdev: &controller.other_evdev,
        },
        connection: ConnectionObservation {
            connection: controller.connection,
            input_bus: controller.input_bus,
            form: controller.form,
        },
        function: FunctionStatus::NotTested,
    }
}

fn driver_observation(controller: &Controller) -> DriverObservation<'_> {
    let interfaces: Vec<InterfaceObservation<'_>> = controller
        .interfaces
        .iter()
        .map(|iface| InterfaceObservation {
            number: iface.number,
            driver: iface.driver.as_deref(),
        })
        .collect();
    let expected_driver = controller
        .expected_driver
        .as_deref()
        .filter(|name| !name.is_empty());
    let mut bound_drivers = Vec::new();
    let mut additional_drivers = Vec::new();
    for iface in &interfaces {
        let Some(driver) = iface.driver else {
            continue;
        };
        push_unique(&mut bound_drivers, driver);
        if expected_driver.is_some_and(|expected| driver != expected) {
            push_unique(&mut additional_drivers, driver);
        }
    }
    let missing_number_count = interfaces
        .iter()
        .filter(|iface| iface.number.is_none())
        .count();
    let duplicate_numbers = duplicate_numbers(&interfaces);
    let comparison = compare_drivers(&interfaces, expected_driver, &bound_drivers);
    DriverObservation {
        binding: controller.driver_binding,
        comparison,
        expected_driver,
        interfaces,
        bound_drivers,
        additional_drivers,
        missing_number_count,
        duplicate_numbers,
    }
}

fn compare_drivers(
    interfaces: &[InterfaceObservation<'_>],
    expected_driver: Option<&str>,
    bound_drivers: &[&str],
) -> DriverComparison {
    if interfaces.is_empty() {
        return DriverComparison::NotApplicable;
    }
    let Some(expected) = expected_driver else {
        return if bound_drivers.is_empty() {
            DriverComparison::NoDriverObserved
        } else {
            DriverComparison::NotApplicable
        };
    };
    if bound_drivers.contains(&expected) {
        DriverComparison::ExpectedObserved
    } else if bound_drivers.is_empty() {
        DriverComparison::NoDriverObserved
    } else {
        DriverComparison::OtherDriverObserved
    }
}

fn duplicate_numbers(interfaces: &[InterfaceObservation<'_>]) -> Vec<u8> {
    let mut seen = Vec::new();
    let mut duplicated = Vec::new();
    for iface in interfaces {
        let Some(number) = iface.number else {
            continue;
        };
        if seen.contains(&number) {
            push_unique_u8(&mut duplicated, number);
        } else {
            seen.push(number);
        }
    }
    duplicated.sort_unstable();
    duplicated
}

fn push_unique<'a>(out: &mut Vec<&'a str>, name: &'a str) {
    if !out.contains(&name) {
        out.push(name);
    }
}

fn push_unique_u8(out: &mut Vec<u8>, number: u8) {
    if !out.contains(&number) {
        out.push(number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Controller, UsbInterface};

    fn iface(
        number: Option<u8>,
        class: u8,
        subclass: u8,
        protocol: u8,
        driver: Option<&str>,
    ) -> UsbInterface {
        UsbInterface {
            number,
            class: Some(class),
            subclass: Some(subclass),
            protocol: Some(protocol),
            driver: driver.map(str::to_string),
        }
    }

    fn base() -> Controller {
        Controller {
            instance: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            name: "GameSir-G7 SE Controller for Xbox".into(),
            catalog_name: Some("GameSir G7 SE".into()),
            input_name: Some("Microsoft Xbox Controller".into()),
            serial: Some("SECRET-SERIAL".into()),
            connection: Connection::Usb,
            input_bus: Connection::Virtual,
            form: Some(DeviceForm::Wired),
            interfaces: vec![iface(Some(0), 0xff, 0x47, 0xd0, Some("xone-wired"))],
            identity: Identity::Known {
                profile_id: "gamesir-g7-se".into(),
            },
            expected_driver: Some("xone-wired".into()),
            driver_binding: DriverBinding::Expected,
            availability: Availability::Available,
            gamepad_evdev: vec!["/dev/input/event30".into()],
            joystick: vec!["/dev/input/js2".into()],
            other_evdev: Vec::new(),
            notes: None,
        }
    }

    #[test]
    fn known_controller_observes_the_expected_driver_and_does_not_verify_function() {
        let controller = base();
        let diag = diagnose_controller(&controller);
        assert_eq!(
            diag.recognition,
            Recognition::Known {
                profile_id: "gamesir-g7-se"
            }
        );
        assert_eq!(diag.catalog_name, Some("GameSir G7 SE"));
        assert_eq!(diag.driver.binding, DriverBinding::Expected);
        assert_eq!(diag.driver.comparison, DriverComparison::ExpectedObserved);
        assert_eq!(diag.driver.bound_drivers, vec!["xone-wired"]);
        assert!(diag.driver.additional_drivers.is_empty());
        assert_eq!(diag.input.availability, Availability::Available);
        assert!(diag.input.gamepad_evdev_reported());
        assert!(diag.input.joystick_reported());
        assert!(!diag.input.other_evdev_reported());
        assert_eq!(diag.connection.connection, Connection::Usb);
        assert_eq!(diag.connection.input_bus, Connection::Virtual);
        assert_eq!(diag.connection.form, Some(DeviceForm::Wired));
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(!diag.observations().contains(&Observation::UnboundInterface));
        assert!(
            !diag
                .observations()
                .contains(&Observation::ExpectedDriverNotObserved)
        );
        assert!(!format!("{diag:?}").contains("SECRET"));
    }

    #[test]
    fn g7_se_expected_driver_keeps_the_unbound_sibling_visible() {
        let mut controller = base();
        controller.interfaces = vec![
            iface(Some(0), 0xff, 0x47, 0xd0, Some("xone-wired")),
            iface(Some(1), 0xff, 0x47, 0xd0, Some("xone-wired")),
            iface(Some(2), 0xff, 0x47, 0xd0, None),
        ];
        let diag = diagnose_controller(&controller);
        assert_eq!(diag.driver.comparison, DriverComparison::ExpectedObserved);
        assert_eq!(diag.driver.binding, DriverBinding::Expected);
        assert_eq!(diag.driver.bound_drivers, vec!["xone-wired"]);
        assert!(diag.driver.additional_drivers.is_empty());
        assert!(
            diag.driver
                .interfaces
                .iter()
                .any(|iface| { iface.number == Some(2) && iface.driver.is_none() })
        );
        assert_eq!(diag.function, FunctionStatus::NotTested);
        let notes = diag.observations();
        assert!(notes.contains(&Observation::UnboundInterface));
        assert!(!notes.contains(&Observation::ExpectedDriverNotObserved));
        assert!(!notes.contains(&Observation::NoGamepadInputRegistered));
    }

    #[test]
    fn wolverine_keeps_usbhid_without_calling_it_a_conflict() {
        let mut controller = base();
        controller.instance = "1-9".into();
        controller.vendor_id = 0x1532;
        controller.product_id = 0x0a57;
        controller.name = "Razer Wolverine V3 Pro for PC".into();
        controller.catalog_name = Some("Razer Wolverine V3 Pro 8K".into());
        controller.input_name = Some("Generic X-Box pad".into());
        controller.input_bus = Connection::Usb;
        controller.identity = Identity::Known {
            profile_id: "razer-wolverine-v3-pro-8k".into(),
        };
        controller.expected_driver = Some("xpad-noone".into());
        controller.interfaces = vec![
            iface(Some(0), 0xff, 0x5d, 0x01, Some("xpad-noone")),
            iface(Some(1), 0x03, 0x01, 0x01, Some("usbhid")),
            iface(Some(2), 0x03, 0x00, 0x00, Some("usbhid")),
        ];
        controller.gamepad_evdev = vec!["/dev/input/event17".into()];
        controller.joystick = vec!["/dev/input/js1".into()];
        controller.other_evdev = vec![
            "/dev/input/event12".into(),
            "/dev/input/event13".into(),
            "/dev/input/event14".into(),
        ];
        let diag = diagnose_controller(&controller);
        assert_eq!(
            diag.recognition,
            Recognition::Known {
                profile_id: "razer-wolverine-v3-pro-8k"
            }
        );
        assert_eq!(diag.driver.comparison, DriverComparison::ExpectedObserved);
        assert_eq!(diag.driver.bound_drivers, vec!["xpad-noone", "usbhid"]);
        assert_eq!(diag.driver.additional_drivers, vec!["usbhid"]);
        assert!(diag.input.other_evdev_reported());
        assert!(diag.input.gamepad_evdev_reported());
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(diag.observations().contains(&Observation::AdditionalDriver));
        assert!(!diag.observations().contains(&Observation::UnboundInterface));
    }

    #[test]
    fn different_bound_driver_is_observed_and_not_a_usability_verdict() {
        let mut controller = base();
        controller.driver_binding = DriverBinding::Missing;
        controller.interfaces = vec![iface(Some(0), 0xff, 0x5d, 0x01, Some("usbhid"))];
        controller.expected_driver = Some("xpad-noone".into());
        let diag = diagnose_controller(&controller);
        assert_eq!(
            diag.driver.comparison,
            DriverComparison::OtherDriverObserved
        );
        assert_eq!(diag.driver.binding, DriverBinding::Missing);
        assert_eq!(diag.driver.bound_drivers, vec!["usbhid"]);
        assert_eq!(diag.driver.additional_drivers, vec!["usbhid"]);
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(
            diag.observations()
                .contains(&Observation::ExpectedDriverNotObserved)
        );
    }

    #[test]
    fn known_controller_with_every_interface_unbound_has_no_driver_observation() {
        let mut controller = base();
        controller.driver_binding = DriverBinding::Unbound;
        controller.interfaces = vec![
            iface(Some(0), 0xff, 0x47, 0xd0, None),
            iface(Some(1), 0xff, 0x47, 0xd0, None),
        ];
        let diag = diagnose_controller(&controller);
        assert_eq!(diag.driver.comparison, DriverComparison::NoDriverObserved);
        assert!(diag.driver.bound_drivers.is_empty());
        assert!(diag.driver.additional_drivers.is_empty());
        assert_eq!(diag.function, FunctionStatus::NotTested);
        let notes = diag.observations();
        assert!(notes.contains(&Observation::ExpectedDriverNotObserved));
        assert!(notes.contains(&Observation::UnboundInterface));
    }

    #[test]
    fn unknown_gamepad_does_not_receive_a_catalog_model() {
        let mut controller = base();
        controller.identity = Identity::Unknown;
        controller.catalog_name = Some("Should Not Be Used".into());
        controller.expected_driver = None;
        controller.driver_binding = DriverBinding::NotApplicable;
        controller.form = None;
        controller.interfaces = vec![iface(Some(0), 0x03, 0x00, 0x00, Some("usbhid"))];
        let diag = diagnose_controller(&controller);
        assert_eq!(diag.recognition, Recognition::Unknown);
        assert_eq!(diag.catalog_name, None);
        assert_eq!(diag.driver.comparison, DriverComparison::NotApplicable);
        assert_eq!(diag.driver.bound_drivers, vec!["usbhid"]);
        assert!(diag.driver.additional_drivers.is_empty());
        assert!(diag.input.gamepad_evdev_reported());
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(diag.observations().contains(&Observation::UnknownIdentity));
        assert!(!diag.observations().contains(&Observation::AdditionalDriver));
    }

    #[test]
    fn ambiguous_identity_keeps_every_candidate() {
        let mut controller = base();
        controller.identity = Identity::Ambiguous {
            profile_ids: vec!["left".into(), "right".into()],
        };
        controller.catalog_name = Some("Left Model".into());
        controller.expected_driver = None;
        controller.driver_binding = DriverBinding::NotApplicable;
        controller.form = None;
        let diag = diagnose_controller(&controller);
        let Identity::Ambiguous { profile_ids } = &controller.identity else {
            panic!("fixture is ambiguous");
        };
        assert_eq!(diag.recognition, Recognition::Ambiguous { profile_ids });
        assert_eq!(diag.catalog_name, None);
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(
            diag.observations()
                .contains(&Observation::AmbiguousIdentity)
        );
        assert!(!matches!(diag.recognition, Recognition::Known { .. }));
    }

    #[test]
    fn bluetooth_input_only_controller_has_no_driver_comparison() {
        let mut controller = base();
        controller.instance = "input:/devices/virtual/bluetooth".into();
        controller.identity = Identity::Unknown;
        controller.catalog_name = None;
        controller.expected_driver = None;
        controller.driver_binding = DriverBinding::NotApplicable;
        controller.connection = Connection::Bluetooth;
        controller.input_bus = Connection::Bluetooth;
        controller.form = None;
        controller.interfaces.clear();
        let diag = diagnose_controller(&controller);
        assert_eq!(diag.recognition, Recognition::Unknown);
        assert_eq!(diag.connection.connection, Connection::Bluetooth);
        assert_eq!(diag.connection.input_bus, Connection::Bluetooth);
        assert_eq!(diag.connection.form, None);
        assert_eq!(diag.driver.comparison, DriverComparison::NotApplicable);
        assert_eq!(diag.driver.binding, DriverBinding::NotApplicable);
        assert!(diag.driver.interfaces.is_empty());
        let notes = diag.observations();
        assert!(notes.contains(&Observation::NoUsbInterfaces));
        assert!(!notes.contains(&Observation::UnboundInterface));
        assert!(!notes.contains(&Observation::ExpectedDriverNotObserved));
        assert_eq!(diag.function, FunctionStatus::NotTested);
    }

    #[test]
    fn no_gamepad_input_stays_separate_from_function_status() {
        let mut controller = base();
        controller.availability = Availability::NoGamepadInput;
        controller.gamepad_evdev.clear();
        controller.joystick.clear();
        let diag = diagnose_controller(&controller);
        assert_eq!(diag.input.availability, Availability::NoGamepadInput);
        assert!(!diag.input.gamepad_evdev_reported());
        assert!(!diag.input.joystick_reported());
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(
            diag.observations()
                .contains(&Observation::NoGamepadInputRegistered)
        );
    }

    #[test]
    fn known_controller_without_usb_interfaces_is_not_called_unbound() {
        let mut controller = base();
        controller.interfaces.clear();
        controller.driver_binding = DriverBinding::NotApplicable;
        let diag = diagnose_controller(&controller);
        assert_eq!(
            diag.recognition,
            Recognition::Known {
                profile_id: "gamesir-g7-se"
            }
        );
        assert_eq!(diag.driver.comparison, DriverComparison::NotApplicable);
        assert!(diag.driver.expected_driver.is_some());
        let notes = diag.observations();
        assert!(notes.contains(&Observation::NoUsbInterfaces));
        assert!(!notes.contains(&Observation::UnboundInterface));
        assert!(!notes.contains(&Observation::ExpectedDriverNotObserved));
    }

    #[test]
    fn joystick_without_gamepad_evdev_is_still_not_a_button_test() {
        let mut controller = base();
        controller.gamepad_evdev.clear();
        controller.joystick = vec!["/dev/input/js0".into()];
        let diag = diagnose_controller(&controller);
        assert!(!diag.input.gamepad_evdev_reported());
        assert!(diag.input.joystick_reported());
        assert_eq!(diag.input.availability, Availability::Available);
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(
            !diag
                .observations()
                .contains(&Observation::NoGamepadInputRegistered)
        );
    }

    #[test]
    fn other_evdev_nodes_do_not_count_as_gamepad_input() {
        let mut controller = base();
        controller.availability = Availability::NoGamepadInput;
        controller.gamepad_evdev.clear();
        controller.joystick.clear();
        controller.other_evdev = vec!["/dev/input/event12".into(), "/dev/input/event13".into()];
        let diag = diagnose_controller(&controller);
        assert!(diag.input.other_evdev_reported());
        assert!(!diag.input.gamepad_evdev_reported());
        assert!(!diag.input.joystick_reported());
        assert_eq!(diag.input.availability, Availability::NoGamepadInput);
        assert_eq!(diag.function, FunctionStatus::NotTested);
    }

    #[test]
    fn duplicate_and_missing_interface_numbers_are_recorded() {
        let mut controller = base();
        controller.interfaces = vec![
            iface(Some(0), 0xff, 0x47, 0xd0, Some("xone-wired")),
            iface(Some(0), 0xff, 0x47, 0xd0, Some("xone-wired")),
            iface(None, 0xff, 0x47, 0xd0, None),
        ];
        let diag = diagnose_controller(&controller);
        assert_eq!(diag.driver.duplicate_numbers, vec![0]);
        assert_eq!(diag.driver.missing_number_count, 1);
        assert_eq!(diag.driver.comparison, DriverComparison::ExpectedObserved);
        let notes = diag.observations();
        assert!(notes.contains(&Observation::InterfaceNumberDuplicated));
        assert!(notes.contains(&Observation::InterfaceNumberMissing));
        assert!(notes.contains(&Observation::UnboundInterface));
    }

    #[test]
    fn empty_catalog_name_does_not_change_a_known_profile_id() {
        let mut controller = base();
        controller.catalog_name = Some(String::new());
        let diag = diagnose_controller(&controller);
        assert_eq!(
            diag.recognition,
            Recognition::Known {
                profile_id: "gamesir-g7-se"
            }
        );
        assert_eq!(diag.catalog_name, None);
        assert!(diag.observations().contains(&Observation::CatalogNameEmpty));
        assert_eq!(diag.function, FunctionStatus::NotTested);
        assert!(diag.input.gamepad_evdev_reported());
    }

    #[test]
    fn discovery_json_stays_free_of_the_text_diagnostic() {
        use crate::model::ObservedUsb;
        use crate::{
            ReportOptions, assemble, builtin_profiles, format_snapshot, format_snapshot_json,
        };

        let usb = ObservedUsb {
            sysname: "1-6".into(),
            vendor_id: 0x3537,
            product_id: 0x1010,
            product: Some("GameSir-G7 SE Controller for Xbox".into()),
            serial: Some("SECRET-SERIAL".into()),
            interfaces: vec![
                iface(Some(0), 0xff, 0x47, 0xd0, Some("xone-wired")),
                iface(Some(1), 0xff, 0x47, 0xd0, Some("xone-wired")),
                iface(Some(2), 0xff, 0x47, 0xd0, None),
            ],
        };
        let snap = assemble(&[usb], &[], &builtin_profiles().unwrap(), "k");
        assert_eq!(snap.controllers.len(), 1);
        let diag = diagnose_controller(&snap.controllers[0]);
        assert_eq!(diag.driver.comparison, DriverComparison::ExpectedObserved);
        assert!(
            diag.driver
                .interfaces
                .iter()
                .any(|iface| { iface.number == Some(2) && iface.driver.is_none() })
        );
        let text = format_snapshot(&snap, ReportOptions::default());
        let json = format_snapshot_json(&snap, ReportOptions::default()).unwrap();
        assert!(text.contains("name: GameSir G7 SE"));
        assert!(text.contains("hardware-name: GameSir-G7 SE Controller for Xbox"));
        assert!(text.contains("diagnostic:"));
        assert!(text.contains("function: not tested"));
        assert!(text.contains("unbound-interfaces: 02"));
        assert!(!text.contains("conflict"));
        assert!(json.contains("\"profile_id\": \"gamesir-g7-se\""));
        assert!(json.contains("\"catalog_name\": \"GameSir G7 SE\""));
        assert!(json.contains("\"name\": \"GameSir-G7 SE Controller for Xbox\""));
        assert!(json.contains("\"driver_binding\": \"expected\""));
        assert!(!json.contains("SECRET-SERIAL"));
        assert!(!json.contains("function_status"));
        assert!(!json.contains("NotTested"));
        assert!(!json.contains("diagnostic"));
    }
}
