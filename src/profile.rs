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
    BadDriver { id: String, driver: String },
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
            Self::EmptyId(index) => write!(f, "controller {index}: id is empty"),
            Self::DuplicateId(id) => write!(f, "profile {id}: duplicate id"),
            Self::EmptyName(id) => write!(f, "profile {id}: name is empty"),
            Self::EmptyDriver(id) => write!(f, "profile {id}: expected_driver is empty"),
            Self::BadDriver { id, driver } => write!(
                f,
                "profile {id}: expected_driver {driver:?} is not a driver name"
            ),
            Self::NoInterfaces(id) => {
                write!(f, "profile {id}: interface selectors are missing")
            }
            Self::BadForm { id, form } => write!(
                f,
                "profile {id}: form {form:?} is not wired, usb-receiver, bluetooth, or unknown"
            ),
            Self::Overlap { a, b } => write!(
                f,
                "profiles {a} and {b}: overlapping interface selectors for the same vendor_id and product_id"
            ),
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
    if !is_driver_name(&expected_driver) {
        return Err(ProfileError::BadDriver {
            id,
            driver: expected_driver,
        });
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

/// A driver name is the basename of a sysfs `driver` symlink. It is compared
/// as text. It is never passed to a shell.
fn is_driver_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
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

    #[test]
    fn validation_errors_name_the_profile_and_the_field() {
        let missing_name = r#"
            version = 1
            [[controller]]
            id = "bare"
            vendor_id = 0x1111
            product_id = 0x2222
            expected_driver = "xpad"
            form = "wired"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
        "#;
        let missing = load_profiles(missing_name).unwrap_err().to_string();
        assert!(missing.contains("name"), "{missing}");

        let bad_id = "version = 1\n[[controller]]\nvendor_id = 0x10000\n";
        let bad_usb = load_profiles(bad_id).unwrap_err().to_string();
        assert!(
            bad_usb.contains("vendor_id") || bad_usb.contains("65536") || bad_usb.contains("u16"),
            "{bad_usb}"
        );

        let not_hex = r#"
            version = 1
            [[controller]]
            id = "wide"
            name = "Wide"
            vendor_id = "zz"
            product_id = 1
            expected_driver = "xpad"
            form = "wired"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
        "#;
        let hex = load_profiles(not_hex).unwrap_err().to_string();
        assert!(hex.contains("vendor_id") || hex.contains("zz"), "{hex}");

        let malformed = load_profiles("version = [\n").unwrap_err().to_string();
        assert!(malformed.contains("profile toml"), "{malformed}");

        let command = r#"
            version = 1
            [[controller]]
            id = "evil"
            name = "Evil"
            vendor_id = 1
            product_id = 2
            expected_driver = "xpad; rm -rf /"
            form = "wired"
            [[controller.interface]]
            class = 1
            subclass = 2
            protocol = 3
        "#;
        match load_profiles(command) {
            Err(ProfileError::BadDriver { id, driver }) => {
                assert_eq!(id, "evil");
                assert!(driver.contains("rm"));
            }
            other => panic!("expected bad driver, got {other:?}"),
        }

        let overlap = load_profiles(
            r#"
            version = 1
            [[controller]]
            id = "left"
            name = "Left"
            vendor_id = 0x10
            product_id = 0x20
            expected_driver = "drv-a"
            form = "wired"
            [[controller.interface]]
            class = 0xff
            subclass = 0x01
            protocol = 0x02
            [[controller]]
            id = "right"
            name = "Right"
            vendor_id = 0x10
            product_id = 0x20
            expected_driver = "drv-b"
            form = "wired"
            [[controller.interface]]
            class = 0xff
            subclass = 0x01
            protocol = 0x02
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            overlap.contains("left") && overlap.contains("right"),
            "{overlap}"
        );
        assert!(overlap.contains("interface"), "{overlap}");

        let bad_form = load_profiles(
            r#"
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
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(
            bad_form.contains("profile x") && bad_form.contains("form"),
            "{bad_form}"
        );

        let unknown = load_profiles(
            r#"
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
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(unknown.contains("plugin"), "{unknown}");

        let version = load_profiles("version = 2\ncontroller = []\n")
            .unwrap_err()
            .to_string();
        assert!(version.contains("version"), "{version}");
    }

    #[test]
    fn one_profile_may_list_several_interfaces() {
        let text = r#"
            version = 1
            [[controller]]
            id = "composite"
            name = "Composite"
            vendor_id = 0x1111
            product_id = 0x2222
            expected_driver = "example-driver"
            form = "wired"
            [[controller.interface]]
            class = 0xff
            subclass = 0x5d
            protocol = 0x01
            [[controller.interface]]
            class = 0x03
            subclass = 0x00
            protocol = 0x00
        "#;
        let profiles = load_profiles(text).expect("composite profile is valid");
        assert_eq!(profiles[0].interfaces.len(), 2);
        assert_eq!(
            identify(
                &usb(
                    0x1111,
                    0x2222,
                    vec![iface(0, 0xff, 0x5d, 0x01), iface(1, 0x03, 0x00, 0x00)]
                ),
                &profiles
            ),
            Identity::Known {
                profile_id: "composite".into()
            }
        );
    }

    #[test]
    fn contributed_catalog_entry_is_identified_without_engine_changes() {
        let mut text = BUILTIN.to_string();
        text.push_str(
            r#"
            [[controller]]
            id = "fixture-synthetic-pad"
            name = "Fixture Synthetic Pad"
            vendor_id = 0x1234
            product_id = 0xabcd
            expected_driver = "fixture-driver"
            form = "wired"
            notes = "Test-only. Not a verified device."

            [[controller.interface]]
            class = 0xff
            subclass = 0x47
            protocol = 0xd0
            "#,
        );
        let profiles = load_profiles(&text).expect("builtin catalog plus one fixture");
        assert_eq!(profiles.len(), 4);
        assert_eq!(builtin_profiles().unwrap().len(), 3);
        assert_eq!(
            identify(
                &usb(0x1234, 0xabcd, vec![iface(0, 0xff, 0x47, 0xd0)]),
                &profiles
            ),
            Identity::Known {
                profile_id: "fixture-synthetic-pad".into()
            }
        );
        assert_eq!(
            identify(
                &usb(0x3537, 0x1010, vec![iface(0, 0xff, 0x47, 0xd0)]),
                &profiles
            ),
            Identity::Known {
                profile_id: "gamesir-g7-se".into()
            }
        );
        assert_eq!(
            identify(
                &usb(0x3537, 0x1033, vec![iface(0, 0xff, 0x5d, 0x01)]),
                &profiles
            ),
            Identity::Known {
                profile_id: "gamesir-g7-pro-8k-receiver".into()
            }
        );
        assert_eq!(
            identify(
                &usb(0x1532, 0x0a57, vec![iface(0, 0xff, 0x5d, 0x01)]),
                &profiles
            ),
            Identity::Known {
                profile_id: "razer-wolverine-v3-pro-8k".into()
            }
        );
        assert!(profiles.iter().all(|profile| profile.product_id != 0x0a59));
    }
}
