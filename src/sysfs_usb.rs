//! Read-only USB enumeration.
//!
//! This module only calls `read_dir`, `read_to_string`, and `read_link`. It
//! never writes sysfs, never opens `bind`/`unbind`, and never opens evdev nodes.

use std::fs;
use std::path::Path;

use crate::model::{ObservedUsb, UsbInterface};

pub(crate) fn read_usb_devices(root: &Path) -> (Vec<ObservedUsb>, Vec<String>) {
    let mut warnings = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(dir) => dir,
        Err(err) => {
            warnings.push(format!("cannot read {}: {err}", root.display()));
            return (Vec::new(), warnings);
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(err) => warnings.push(format!("reading {root:?}: {err}")),
        }
    }
    names.sort();
    let mut devices = Vec::new();
    for name in names.iter().filter(|name| !name.contains(':')) {
        match read_device(root, name, &names) {
            Ok(Some(device)) => devices.push(device),
            Ok(None) => {}
            Err(warning) => warnings.push(warning),
        }
    }
    (devices, warnings)
}

fn read_device(
    root: &Path,
    sysname: &str,
    names: &[String],
) -> Result<Option<ObservedUsb>, String> {
    let dir = root.join(sysname);
    let vendor_path = dir.join("idVendor");
    let Some(vendor_text) = read_attr(&vendor_path) else {
        return Ok(None);
    };
    let vendor_id = u16::from_str_radix(&vendor_text, 16)
        .map_err(|_| format!("{} has invalid idVendor {vendor_text}", dir.display()))?;
    let Some(product_text) = read_attr(&dir.join("idProduct")) else {
        return Err(format!("{} has idVendor but no idProduct", dir.display()));
    };
    let product_id = u16::from_str_radix(&product_text, 16)
        .map_err(|_| format!("{} has invalid idProduct {product_text}", dir.display()))?;
    if read_hex_u8(&dir.join("bDeviceClass")) == Some(0x09) {
        return Ok(None);
    }

    let prefix = format!("{sysname}:");
    let mut iface_names: Vec<&str> = names
        .iter()
        .filter(|name| name.starts_with(&prefix))
        .map(String::as_str)
        .collect();
    iface_names.sort_by_key(|name| interface_sort_key(name));

    let mut interfaces = Vec::with_capacity(iface_names.len());
    for iface_name in iface_names {
        interfaces.push(read_interface(&root.join(iface_name)));
    }

    Ok(Some(ObservedUsb {
        sysname: sysname.to_string(),
        vendor_id,
        product_id,
        product: read_attr(&dir.join("product")),
        serial: read_attr(&dir.join("serial")),
        interfaces,
    }))
}

fn read_interface(dir: &Path) -> UsbInterface {
    UsbInterface {
        number: read_hex_u8(&dir.join("bInterfaceNumber")),
        class: read_hex_u8(&dir.join("bInterfaceClass")),
        subclass: read_hex_u8(&dir.join("bInterfaceSubClass")),
        protocol: read_hex_u8(&dir.join("bInterfaceProtocol")),
        driver: read_driver(&dir.join("driver")),
    }
}

fn read_driver(path: &Path) -> Option<String> {
    let target = fs::read_link(path).ok()?;
    let name = target.file_name()?.to_string_lossy().into_owned();
    if name.is_empty() { None } else { Some(name) }
}

fn interface_sort_key(name: &str) -> (u8, &str) {
    let number = name
        .rsplit([':', '.'])
        .next()
        .and_then(|text| u8::from_str_radix(text, 16).ok())
        .unwrap_or(u8::MAX);
    (number, name)
}

fn read_attr(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn read_hex_u8(path: &Path) -> Option<u8> {
    let text = read_attr(path)?;
    u8::from_str_radix(&text, 16).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("padcompat-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn reads_interfaces_drivers_and_missing_attributes() {
        let tmp = TempDir::new();
        let root = tmp.path();
        write(&root.join("1-6/idVendor"), "3537\n");
        write(&root.join("1-6/idProduct"), "1010\n");
        write(&root.join("1-6/bDeviceClass"), "ff\n");
        write(&root.join("1-6/serial"), "TESTSERIAL\n");
        write(&root.join("1-6:1.0/bInterfaceNumber"), "00\n");
        write(&root.join("1-6:1.0/bInterfaceClass"), "ff\n");
        write(&root.join("1-6:1.0/bInterfaceSubClass"), "47\n");
        write(&root.join("1-6:1.0/bInterfaceProtocol"), "d0\n");
        symlink(
            "../../../../bus/usb/drivers/xone-wired",
            root.join("1-6:1.0/driver"),
        )
        .unwrap();
        write(&root.join("1-6:1.2/bInterfaceNumber"), "02\n");
        write(&root.join("1-6:1.2/bInterfaceClass"), "ff\n");
        write(&root.join("1-6:1.2/bInterfaceSubClass"), "47\n");
        write(&root.join("1-6:1.2/bInterfaceProtocol"), "d0\n");
        write(&root.join("usb1/idVendor"), "1d6b\n");
        write(&root.join("usb1/idProduct"), "0002\n");
        write(&root.join("usb1/bDeviceClass"), "09\n");
        write(&root.join("1-2/idVendor"), "zzzz\n");
        write(&root.join("1-2/idProduct"), "0001\n");
        write(&root.join("1-3/idVendor"), "046d\n");

        let (devices, warnings) = read_usb_devices(root);
        assert_eq!(devices.len(), 1, "{warnings:?}");
        let device = &devices[0];
        assert_eq!(device.sysname, "1-6");
        assert_eq!(device.vendor_id, 0x3537);
        assert_eq!(device.product_id, 0x1010);
        assert_eq!(device.product, None);
        assert_eq!(device.serial.as_deref(), Some("TESTSERIAL"));
        assert_eq!(device.interfaces.len(), 2);
        assert_eq!(device.interfaces[0].number, Some(0));
        assert_eq!(device.interfaces[0].class, Some(0xff));
        assert_eq!(device.interfaces[0].subclass, Some(0x47));
        assert_eq!(device.interfaces[0].protocol, Some(0xd0));
        assert_eq!(device.interfaces[0].driver.as_deref(), Some("xone-wired"));
        assert_eq!(device.interfaces[1].number, Some(2));
        assert_eq!(device.interfaces[1].driver, None);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("invalid idVendor"))
        );
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("no idProduct"))
        );
    }

    #[test]
    fn missing_sysfs_root_is_a_warning() {
        let (devices, warnings) = read_usb_devices(Path::new("/definitely/not/padcompat-sysfs"));
        assert!(devices.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
