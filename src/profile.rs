use std::fmt;

use crate::model::{DeviceForm, Identity, ObservedUsb, UsbInterface};

/// One interface identity inside a catalog profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceSelector {
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    /// When set, only this interface number matches. `None` matches any number.
    pub number: Option<u8>,
}

/// One declarative controller profile. This is the catalog record a future
/// community database has to validate. There is no plugin hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerProfile {
    pub id: String,
    pub name: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub expected_driver: String,
    pub form: DeviceForm,
    pub interfaces: Vec<InterfaceSelector>,
    pub notes: Option<String>,
}

#[derive(Debug)]
pub enum ProfileError {
    Toml(String),
    UnsupportedVersion(u32),
    EmptyId(usize),
    DuplicateId(String),
    EmptyName(String),
    EmptyDriver(String),
    NoInterfaces(String),
    BadForm { id: String, form: String },
    Overlap { a: String, b: String },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(msg) => write!(f, "profile toml: {msg}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported profile catalog version {version}")
            }
            Self::EmptyId(index) => write!(f, "controller {index} has an empty id"),
            Self::DuplicateId(id) => write!(f, "duplicate profile id {id}"),
            Self::EmptyName(id) => write!(f, "profile {id} has an empty name"),
            Self::EmptyDriver(id) => write!(f, "profile {id} has an empty expected_driver"),
            Self::NoInterfaces(id) => write!(f, "profile {id} has no interface selectors"),
            Self::BadForm { id, form } => write!(f, "profile {id} has unknown form {form}"),
            Self::Overlap { a, b } => {
                write!(f, "profiles {a} and {b} match the same usb identity")
            }
        }
    }
}

impl std::error::Error for ProfileError {}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    version: u32,
    #[serde(default)]
    controller: Vec<RawController>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawController {
    id: String,
    name: String,
    vendor_id: u16,
    product_id: u16,
    expected_driver: String,
    form: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    interface: Vec<RawInterface>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInterface {
    class: u8,
    subclass: u8,
    protocol: u8,
    number: Option<u8>,
}

const BUILTIN: &str = include_str!("../profiles/controllers.toml");

pub fn builtin_profiles() -> Result<Vec<ControllerProfile>, ProfileError> {
    load_profiles(BUILTIN)
}

pub fn load_profiles(text: &str) -> Result<Vec<ControllerProfile>, ProfileError> {
    let file: CatalogFile =
        toml::from_str(text).map_err(|err| ProfileError::Toml(err.to_string()))?;
    if file.version != 1 {
        return Err(ProfileError::UnsupportedVersion(file.version));
    }
    let mut profiles = Vec::with_capacity(file.controller.len());
    for (index, raw) in file.controller.into_iter().enumerate() {
        profiles.push(convert(index, raw)?);
    }
    validate(&profiles)?;
    Ok(profiles)
}

/// Match `(vendor, product, interface)` against the catalog.
///
/// Vendor and product id are required and are not sufficient. At least one
/// observed interface must match a selector. Several matching profiles produce
/// [`Identity::Ambiguous`] instead of a guess.
pub fn identify(usb: &ObservedUsb, profiles: &[ControllerProfile]) -> Identity {
    let mut ids: Vec<String> = profiles
        .iter()
        .filter(|profile| profile_matches(profile, usb))
        .map(|profile| profile.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    match ids.len() {
        0 => Identity::Unknown,
        1 => Identity::Known {
            profile_id: ids.remove(0),
        },
        _ => Identity::Ambiguous { profile_ids: ids },
    }
}

fn convert(index: usize, raw: RawController) -> Result<ControllerProfile, ProfileError> {
    let id = raw.id.trim().to_string();
    if id.is_empty() {
        return Err(ProfileError::EmptyId(index));
    }
    let name = raw.name.trim().to_string();
    if name.is_empty() {
        return Err(ProfileError::EmptyName(id));
    }
    let expected_driver = raw.expected_driver.trim().to_string();
    if expected_driver.is_empty() {
        return Err(ProfileError::EmptyDriver(id));
    }
    if raw.interface.is_empty() {
        return Err(ProfileError::NoInterfaces(id));
    }
    let Some(form) = DeviceForm::parse(raw.form.trim()) else {
        return Err(ProfileError::BadForm { id, form: raw.form });
    };
    let notes = raw.notes.and_then(|notes| {
        let notes = notes.trim().to_string();
        if notes.is_empty() { None } else { Some(notes) }
    });
    let interfaces = raw
        .interface
        .into_iter()
        .map(|iface| InterfaceSelector {
            class: iface.class,
            subclass: iface.subclass,
            protocol: iface.protocol,
            number: iface.number,
        })
        .collect();
    Ok(ControllerProfile {
        id,
        name,
        vendor_id: raw.vendor_id,
        product_id: raw.product_id,
        expected_driver,
        form,
        interfaces,
        notes,
    })
}

fn validate(profiles: &[ControllerProfile]) -> Result<(), ProfileError> {
    let mut seen = Vec::new();
    for profile in profiles {
        if seen.iter().any(|id: &String| id == &profile.id) {
            return Err(ProfileError::DuplicateId(profile.id.clone()));
        }
        seen.push(profile.id.clone());
    }
    for (index, left) in profiles.iter().enumerate() {
        for right in profiles.iter().skip(index + 1) {
            if conflicts(left, right) {
                return Err(ProfileError::Overlap {
                    a: left.id.clone(),
                    b: right.id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn conflicts(left: &ControllerProfile, right: &ControllerProfile) -> bool {
    if left.vendor_id != right.vendor_id || left.product_id != right.product_id {
        return false;
    }
    left.interfaces.iter().any(|left_sel| {
        right
            .interfaces
            .iter()
            .any(|right_sel| selectors_overlap(left_sel, right_sel))
    })
}

fn selectors_overlap(left: &InterfaceSelector, right: &InterfaceSelector) -> bool {
    if left.class != right.class
        || left.subclass != right.subclass
        || left.protocol != right.protocol
    {
        return false;
    }
    match (left.number, right.number) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

fn profile_matches(profile: &ControllerProfile, usb: &ObservedUsb) -> bool {
    if profile.vendor_id != usb.vendor_id || profile.product_id != usb.product_id {
        return false;
    }
    usb.interfaces.iter().any(|iface| {
        profile
            .interfaces
            .iter()
            .any(|sel| iface_matches(sel, iface))
    })
}

fn iface_matches(selector: &InterfaceSelector, iface: &UsbInterface) -> bool {
    let (Some(class), Some(subclass), Some(protocol)) =
        (iface.class, iface.subclass, iface.protocol)
    else {
        return false;
    };
    if selector.class != class || selector.subclass != subclass || selector.protocol != protocol {
        return false;
    }
    match selector.number {
        Some(number) => iface.number == Some(number),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb(vid: u16, pid: u16, interfaces: Vec<UsbInterface>) -> ObservedUsb {
        ObservedUsb {
            sysname: "1-1".into(),
            vendor_id: vid,
            product_id: pid,
            product: None,
            serial: None,
            interfaces,
        }
    }

    fn iface(number: u8, class: u8, subclass: u8, protocol: u8) -> UsbInterface {
        UsbInterface {
            number: Some(number),
            class: Some(class),
            subclass: Some(subclass),
            protocol: Some(protocol),
            driver: None,
        }
    }

    #[test]
    fn builtin_catalog_loads_three_known_controllers() {
        let profiles = builtin_profiles().expect("builtin catalog");
        let ids: Vec<_> = profiles.iter().map(|profile| profile.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "gamesir-g7-se",
                "gamesir-g7-pro-8k-receiver",
                "razer-wolverine-v3-pro-8k",
            ]
        );
        assert_eq!(profiles[1].name, "GameSir G7 Pro 8K");
        assert_eq!(profiles[1].form, DeviceForm::UsbReceiver);
        assert_eq!(profiles[1].interfaces.len(), 1);
        assert_eq!(profiles[1].interfaces[0].protocol, 0x01);
        assert!(profiles[1].notes.is_none());
        assert_eq!(profiles[0].form, DeviceForm::Wired);
        assert_eq!(profiles[2].form, DeviceForm::Wired);
        assert_eq!(profiles[2].product_id, 0x0a57);
        assert!(profiles.iter().all(|profile| profile.product_id != 0x0a59));
    }

    #[test]
    fn known_controllers_match_on_interface_not_vendor_product_alone() {
        let profiles = builtin_profiles().unwrap();

        let g7 = usb(0x3537, 0x1010, vec![iface(0, 0xff, 0x47, 0xd0)]);
        assert_eq!(
            identify(&g7, &profiles),
            Identity::Known {
                profile_id: "gamesir-g7-se".into()
            }
        );

        let g7_wrong_iface = usb(0x3537, 0x1010, vec![iface(0, 0xff, 0x5d, 0x01)]);
        assert_eq!(identify(&g7_wrong_iface, &profiles), Identity::Unknown);

        let g7_missing_class = usb(
            0x3537,
            0x1010,
            vec![UsbInterface {
                number: Some(0),
                class: None,
                subclass: Some(0x47),
                protocol: Some(0xd0),
                driver: None,
            }],
        );
        assert_eq!(identify(&g7_missing_class, &profiles), Identity::Unknown);

        let pro_observed = usb(0x3537, 0x1033, vec![iface(0, 0xff, 0x5d, 0x01)]);
        let pro_unseen_wireless_protocol = usb(0x3537, 0x1033, vec![iface(0, 0xff, 0x5d, 0x81)]);
        let pro_gip = usb(0x3537, 0x1033, vec![iface(0, 0xff, 0x47, 0xd0)]);
        assert_eq!(
            identify(&pro_observed, &profiles),
            Identity::Known {
                profile_id: "gamesir-g7-pro-8k-receiver".into()
            }
        );
        assert_eq!(
            identify(&pro_unseen_wireless_protocol, &profiles),
            Identity::Unknown
        );
        assert_eq!(identify(&pro_gip, &profiles), Identity::Unknown);

        let razer = usb(
            0x1532,
            0x0a57,
            vec![iface(1, 0x03, 0x01, 0x01), iface(0, 0xff, 0x5d, 0x01)],
        );
        assert_eq!(
            identify(&razer, &profiles),
            Identity::Known {
                profile_id: "razer-wolverine-v3-pro-8k".into()
            }
        );
        let razer_hid_only = usb(0x1532, 0x0a57, vec![iface(1, 0x03, 0x01, 0x01)]);
        assert_eq!(identify(&razer_hid_only, &profiles), Identity::Unknown);
    }

    #[test]
    fn interface_number_narrows_a_match() {
        let text = r#"
            version = 1
            [[controller]]
            id = "iface-0"
            name = "Iface 0"
            vendor_id = 0x3537
            product_id = 0x1033
            expected_driver = "xpad-noone"
            form = "usb-receiver"
            [[controller.interface]]
            class = 0xff
            subclass = 0x5d
            protocol = 0x01
            number = 0

            [[controller]]
            id = "iface-1"
            name = "Iface 1"
            vendor_id = 0x3537
            product_id = 0x1033
            expected_driver = "xpad-noone"
            form = "usb-receiver"
            [[controller.interface]]
            class = 0xff
            subclass = 0x5d
            protocol = 0x01
            number = 1
        "#;
        let profiles = load_profiles(text).unwrap();
        assert_eq!(
            identify(
                &usb(0x3537, 0x1033, vec![iface(0, 0xff, 0x5d, 0x01)]),
                &profiles
            ),
            Identity::Known {
                profile_id: "iface-0".into()
            }
        );
        assert_eq!(
            identify(
                &usb(0x3537, 0x1033, vec![iface(1, 0xff, 0x5d, 0x01)]),
                &profiles
            ),
            Identity::Known {
                profile_id: "iface-1".into()
            }
        );
    }

    #[test]
    fn ambiguous_profiles_are_reported_instead_of_picked() {
        let selector = InterfaceSelector {
            class: 0xff,
            subclass: 0x47,
            protocol: 0xd0,
            number: None,
        };
        let profiles = vec![
            ControllerProfile {
                id: "b".into(),
                name: "B".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: DeviceForm::Wired,
                interfaces: vec![selector.clone()],
                notes: None,
            },
            ControllerProfile {
                id: "a".into(),
                name: "A".into(),
                vendor_id: 0x3537,
                product_id: 0x1010,
                expected_driver: "xone-wired".into(),
                form: DeviceForm::Wired,
                interfaces: vec![selector],
                notes: None,
            },
        ];
        assert_eq!(
            identify(
                &usb(0x3537, 0x1010, vec![iface(0, 0xff, 0x47, 0xd0)]),
                &profiles
            ),
            Identity::Ambiguous {
                profile_ids: vec!["a".into(), "b".into()]
            }
        );
    }

    #[test]
    fn loader_rejects_incomplete_and_overlapping_profiles() {
        let missing_iface = r#"
            version = 1
            [[controller]]
            id = "bare"
            name = "Bare"
            vendor_id = 0x1111
            product_id = 0x2222
            expected_driver = "xpad"
            form = "wired"
        "#;
        assert!(matches!(
            load_profiles(missing_iface),
            Err(ProfileError::NoInterfaces(_))
        ));

        let duplicate = r#"
            version = 1
            [[controller]]
            id = "same"
            name = "One"
            vendor_id = 0x1
            product_id = 0x2
            expected_driver = "drv"
            form = "wired"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
            [[controller]]
            id = "same"
            name = "Two"
            vendor_id = 0x3
            product_id = 0x4
            expected_driver = "drv"
            form = "wired"
            [[controller.interface]]
            class = 4
            subclass = 5
            protocol = 6
        "#;
        assert!(matches!(
            load_profiles(duplicate),
            Err(ProfileError::DuplicateId(_))
        ));

        let overlap = r#"
            version = 1
            [[controller]]
            id = "left"
            name = "Left"
            vendor_id = 0x3537
            product_id = 0x1010
            expected_driver = "xone-wired"
            form = "wired"
            [[controller.interface]]
            class = 0xff
            subclass = 0x47
            protocol = 0xd0
            [[controller]]
            id = "right"
            name = "Right"
            vendor_id = 0x3537
            product_id = 0x1010
            expected_driver = "xone-wired"
            form = "wired"
            [[controller.interface]]
            class = 0xff
            subclass = 0x47
            protocol = 0xd0
            number = 0
        "#;
        assert!(matches!(
            load_profiles(overlap),
            Err(ProfileError::Overlap { .. })
        ));

        let bad_form = r#"
            version = 1
            [[controller]]
            id = "x"
            name = "X"
            vendor_id = 1
            product_id = 2
            expected_driver = "drv"
            form = "dongle"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
        "#;
        assert!(matches!(
            load_profiles(bad_form),
            Err(ProfileError::BadForm { .. })
        ));

        let unknown_field = r#"
            version = 1
            [[controller]]
            id = "x"
            name = "X"
            vendor_id = 1
            product_id = 2
            expected_driver = "drv"
            form = "wired"
            plugin = "nope"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
        "#;
        assert!(matches!(
            load_profiles(unknown_field),
            Err(ProfileError::Toml(_))
        ));

        assert!(matches!(
            load_profiles("version = 2\ncontroller = []\n"),
            Err(ProfileError::UnsupportedVersion(2))
        ));

        let empty_driver = r#"
            version = 1
            [[controller]]
            id = "x"
            name = "X"
            vendor_id = 1
            product_id = 2
            expected_driver = "  "
            form = "wired"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
        "#;
        assert!(matches!(
            load_profiles(empty_driver),
            Err(ProfileError::EmptyDriver(_))
        ));
    }
}
