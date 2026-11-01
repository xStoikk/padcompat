use serde::{Serialize, Serializer};

fn ser_hex_u16<S: Serializer>(value: &u16, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{value:04x}"))
}

fn ser_opt_hex_u8<S: Serializer>(value: &Option<u8>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serializer.serialize_str(&format!("{value:02x}")),
        None => serializer.serialize_none(),
    }
}

/// How the kernel exposes the device, or how the USB parent is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Usb,
    Bluetooth,
    Virtual,
    Unknown,
}

impl Connection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Bluetooth => "bluetooth",
            Self::Virtual => "virtual",
            Self::Unknown => "unknown",
        }
    }
}

/// Catalog fact about the product. This is not a kernel observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceForm {
    Wired,
    UsbReceiver,
    Bluetooth,
    Unknown,
}

impl DeviceForm {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "wired" => Some(Self::Wired),
            "usb-receiver" => Some(Self::UsbReceiver),
            "bluetooth" => Some(Self::Bluetooth),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wired => "wired",
            Self::UsbReceiver => "usb-receiver",
            Self::Bluetooth => "bluetooth",
            Self::Unknown => "unknown",
        }
    }
}

/// Whether a known profile's expected interface driver is bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverBinding {
    Expected,
    Missing,
    Unbound,
    NotApplicable,
}

/// Whether a gamepad evdev or joystick node is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    Available,
    NoGamepadInput,
}

/// Result of matching one USB device against the profile catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Identity {
    Known { profile_id: String },
    Ambiguous { profile_ids: Vec<String> },
    Unknown,
}

/// One USB interface observed from sysfs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsbInterface {
    #[serde(serialize_with = "ser_opt_hex_u8")]
    pub number: Option<u8>,
    #[serde(serialize_with = "ser_opt_hex_u8")]
    pub class: Option<u8>,
    #[serde(serialize_with = "ser_opt_hex_u8")]
    pub subclass: Option<u8>,
    #[serde(serialize_with = "ser_opt_hex_u8")]
    pub protocol: Option<u8>,
    /// Basename of the interface `driver` symlink. `None` means unbound.
    pub driver: Option<String>,
}

/// One USB device, collapsed from its interfaces. Not yet filtered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedUsb {
    pub sysname: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub interfaces: Vec<UsbInterface>,
}

/// One entry from `/proc/bus/input/devices`.
///
/// `key_bits` and `abs_bits` are kernel bitmap words with the most significant
/// word first, matching the proc file. On a 64-bit kernel each word is a `u64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputNode {
    pub name: String,
    pub phys: String,
    pub sysfs: String,
    pub bustype: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub handlers: Vec<String>,
    pub key_bits: Vec<u64>,
    pub abs_bits: Vec<u64>,
}

/// One controller-like device after profile matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Controller {
    /// USB topology name (`1-6`) or `input:` plus the input parent path.
    pub instance: String,
    #[serde(serialize_with = "ser_hex_u16")]
    pub vendor_id: u16,
    #[serde(serialize_with = "ser_hex_u16")]
    pub product_id: u16,
    pub name: String,
    /// Catalog name when a single profile matched. Independent of the USB product string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_name: Option<String>,
    pub input_name: Option<String>,
    /// Present in memory for correlation. Omitted from reports unless opted in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// Physical attachment. USB parent wins over a virtual input bus.
    pub connection: Connection,
    /// Bus of the gamepad evdev node. xone reports `virtual` under a USB device.
    pub input_bus: Connection,
    pub form: Option<DeviceForm>,
    pub interfaces: Vec<UsbInterface>,
    pub identity: Identity,
    pub expected_driver: Option<String>,
    pub driver_binding: DriverBinding,
    pub availability: Availability,
    pub gamepad_evdev: Vec<String>,
    pub joystick: Vec<String>,
    pub other_evdev: Vec<String>,
    pub notes: Option<String>,
}

/// A point-in-time discovery result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    pub kernel_release: String,
    pub controllers: Vec<Controller>,
    pub warnings: Vec<String>,
}

/// Difference between two snapshots. Instance id is the USB sysname, or the
/// input parent path for devices that have no USB parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DeviceEvent {
    Connected(Controller),
    Disconnected {
        instance: String,
        name: String,
        #[serde(serialize_with = "ser_hex_u16")]
        vendor_id: u16,
        #[serde(serialize_with = "ser_hex_u16")]
        product_id: u16,
    },
    Updated(Controller),
}
