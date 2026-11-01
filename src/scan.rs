use std::fs;
use std::io;
use std::path::PathBuf;

use crate::assemble::assemble;
use crate::model::{Controller, Snapshot};
use crate::proc_input::parse_proc_input;
use crate::profile::ControllerProfile;
use crate::sysfs_usb::read_usb_devices;

/// System paths used by a scan. Tests point these at fixtures.
#[derive(Debug, Clone)]
pub struct ScanPaths {
    pub usb_devices: PathBuf,
    pub proc_input: PathBuf,
    pub kernel_release: PathBuf,
}

impl ScanPaths {
    pub fn system() -> Self {
        Self {
            usb_devices: PathBuf::from("/sys/bus/usb/devices"),
            proc_input: PathBuf::from("/proc/bus/input/devices"),
            kernel_release: PathBuf::from("/proc/sys/kernel/osrelease"),
        }
    }
}

pub fn scan_system(profiles: &[ControllerProfile]) -> io::Result<Snapshot> {
    scan_with(&ScanPaths::system(), profiles, true)
}

pub fn scan_with(
    paths: &ScanPaths,
    profiles: &[ControllerProfile],
    stat_devnodes: bool,
) -> io::Result<Snapshot> {
    let kernel_release = fs::read_to_string(&paths.kernel_release)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let proc_text = fs::read_to_string(&paths.proc_input).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!("read {}: {err}", paths.proc_input.display()),
        )
    })?;
    let inputs = parse_proc_input(&proc_text);
    let (usbs, mut warnings) = read_usb_devices(&paths.usb_devices);
    let mut snapshot = assemble(&usbs, &inputs, profiles, &kernel_release);
    if stat_devnodes {
        warnings.extend(missing_devnodes(&snapshot.controllers));
    }
    warnings.append(&mut snapshot.warnings);
    snapshot.warnings = warnings;
    Ok(snapshot)
}

fn missing_devnodes(controllers: &[Controller]) -> Vec<String> {
    let mut warnings = Vec::new();
    for controller in controllers {
        for node in controller
            .gamepad_evdev
            .iter()
            .chain(&controller.joystick)
            .chain(&controller.other_evdev)
        {
            if fs::metadata(node).is_err() {
                warnings.push(format!("{} missing devnode {node}", controller.instance));
            }
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::model::{Availability, DriverBinding, Identity};
    use crate::profile::builtin_profiles;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("padcompat-scan-{nanos}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
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
    fn fixture_sysfs_and_proc_round_trip() {
        let tmp = TempDir::new();
        let usb = tmp.0.join("usb");
        write(&usb.join("1-6/idVendor"), "3537\n");
        write(&usb.join("1-6/idProduct"), "1010\n");
        write(
            &usb.join("1-6/product"),
            "GameSir-G7 SE Controller for Xbox\n",
        );
        write(&usb.join("1-6:1.0/bInterfaceNumber"), "00\n");
        write(&usb.join("1-6:1.0/bInterfaceClass"), "ff\n");
        write(&usb.join("1-6:1.0/bInterfaceSubClass"), "47\n");
        write(&usb.join("1-6:1.0/bInterfaceProtocol"), "d0\n");
        symlink("somewhere/xone-wired", usb.join("1-6:1.0/driver")).unwrap();
        write(&usb.join("1-2/idVendor"), "046d\n");
        write(&usb.join("1-2/idProduct"), "c336\n");
        write(&usb.join("1-2/product"), "Keyboard\n");
        write(&usb.join("1-2:1.0/bInterfaceNumber"), "00\n");
        write(&usb.join("1-2:1.0/bInterfaceClass"), "03\n");
        write(&usb.join("1-2:1.0/bInterfaceSubClass"), "01\n");
        write(&usb.join("1-2:1.0/bInterfaceProtocol"), "01\n");

        let proc_input = tmp.0.join("devices");
        write(
            &proc_input,
            "\
I: Bus=0006 Vendor=3537 Product=1010 Version=0102
N: Name=\"Microsoft Xbox Controller\"
P: Phys=gip0.0/input0
S: Sysfs=/devices/pci/usb1/1-6/1-6:1.0/gip0/input/input4
H: Handlers=event30 js2
B: KEY=7cdb000000000000 0 0 0 0
B: ABS=3003f

I: Bus=0003 Vendor=046d Product=c336 Version=0111
N: Name=\"Keyboard\"
P: Phys=usb-keyboard
S: Sysfs=/devices/pci/usb1/1-2/1-2:1.0/input/input8
H: Handlers=event5
B: KEY=1
",
        );
        write(&tmp.0.join("kernel"), "fixture-kernel\n");

        let snap = scan_with(
            &ScanPaths {
                usb_devices: usb,
                proc_input,
                kernel_release: tmp.0.join("kernel"),
            },
            &builtin_profiles().unwrap(),
            false,
        )
        .unwrap();
        assert_eq!(snap.kernel_release, "fixture-kernel");
        assert_eq!(snap.controllers.len(), 1);
        assert_eq!(
            snap.controllers[0].identity,
            Identity::Known {
                profile_id: "gamesir-g7-se".into()
            }
        );
        assert_eq!(snap.controllers[0].driver_binding, DriverBinding::Expected);
        assert_eq!(snap.controllers[0].availability, Availability::Available);
        assert_eq!(
            snap.controllers[0].joystick,
            vec!["/dev/input/js2".to_string()]
        );
    }
}
