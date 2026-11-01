//! Read-only discovery of Linux gamepads.
//!
//! The crate reads sysfs and `/proc/bus/input/devices`, matches a declarative
//! profile catalog, and can wait for udev device events. It does not open
//! evdev devices, write sysfs, or manage drivers.

mod assemble;
mod bits;
mod hotplug;
mod model;
mod proc_input;
mod profile;
mod report;
mod scan;
mod sysfs_usb;

pub use assemble::{assemble, diff};
pub use hotplug::{WatchStop, watch};
pub use model::{
    Availability, Connection, Controller, DeviceEvent, DeviceForm, DriverBinding, Identity,
    InputNode, ObservedUsb, Snapshot, UsbInterface,
};
pub use proc_input::parse_proc_input;
pub use profile::{
    ControllerProfile, InterfaceSelector, ProfileError, builtin_profiles, identify, load_profiles,
};
pub use report::{
    ReportOptions, format_event, format_event_json, format_snapshot, format_snapshot_json,
};
pub use scan::{ScanPaths, scan_system, scan_with};
