# Contributing a controller profile

padcompat recognizes controllers from `profiles/controllers.toml`. Add a `[[controller]]` table. Do not change the Rust discovery code for a new id, and do not add a plugin.

A profile is not proof that a pad works on every distribution. These are different facts:

| Fact | What it means |
| --- | --- |
| Recognition | Vendor id, product id, and one interface selector match a profile. |
| Driver binding | At runtime, an interface `driver` symlink basename equals `expected_driver`. |
| Registered gamepad input | A gamepad evdev node or `js*` node exists (`status: available`). |
| Hands-on testing | Someone pressed the controls. The catalog does not record this. |

`expected_driver` is the interface driver you observed. It is not a shell command. padcompat never installs, loads, or runs it. The built-in `xpad-noone` rows were observed on one CachyOS machine with an out-of-tree driver. Copying those rows onto another install is not a recommendation to install that driver.

`connection` in a report is the kernel bus. `form` in the catalog is the product link: `wired`, `usb-receiver`, `bluetooth`, or `unknown`. A wired pad and a dongle are both `connection: usb` when the kernel enumerates them on USB.

## Collect the descriptors

Use read-only sysfs. Do not unbind a driver, reload a module, or change udev rules.

Find the device directory under `/sys/bus/usb/devices/` whose `idVendor` and `idProduct` match the pad. Ignore names that contain `:`. Those are interfaces.

```sh
cat /sys/bus/usb/devices/1-7/idVendor
cat /sys/bus/usb/devices/1-7/idProduct
for iface in /sys/bus/usb/devices/1-7:*; do
  printf '%s class=%s subclass=%s protocol=%s driver=%s\n' \
    "$(basename "$iface")" \
    "$(cat "$iface/bInterfaceClass")" \
    "$(cat "$iface/bInterfaceSubClass")" \
    "$(cat "$iface/bInterfaceProtocol")" \
    "$(basename "$(readlink "$iface/driver" 2>/dev/null)" 2>/dev/null || echo unbound)"
done
```

Replace `1-7` with the directory you found. Class, subclass, and protocol are hex. The driver value is the symlink basename (`xone-wired`), not the module name (`xone_wired`).

Match on the interface that identifies the gamepad. A composite device may also expose HID interfaces. You do not have to list those. Listing them makes a HID-only enumeration of the same ids match the profile. One profile may list several selectors when the same product really uses more than one identifying descriptor. Two profiles must not be able to match the same interface of the same ids.

Do not add a wireless id that you have not plugged in. `1532:0a59` is not in the catalog for that reason.

## Minimum profile

Required fields: `id`, `name`, `vendor_id`, `product_id`, `expected_driver`, `form`, and at least one `interface` with `class`, `subclass`, and `protocol`. `interface.number` is optional. `notes` is optional free text, not a certificate.

`id` is a stable slug. `name` is the human-readable model. `expected_driver` is a single driver name: letters, digits, `.`, `_`, and `-`. No spaces, slashes, or shell syntax. Unknown fields are rejected.

## Example

This identity is fake. Do not copy it into `profiles/controllers.toml`.

```toml
[[controller]]
id = "example-not-a-real-pad"
name = "Example Not A Real Pad"
vendor_id = 0x0001
product_id = 0x0001
expected_driver = "example-driver"
form = "wired"

[[controller.interface]]
class = 0xff
subclass = 0x00
protocol = 0x00
```

## Check it

```sh
cargo run -- profiles validate
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

`cargo test` does not scan your controllers. `tests/live_scan.rs` is ignored unless you ask for it:

```sh
cargo test --test live_scan -- --ignored
```

That command fails if none of `3537:1010`, `3537:1033`, or `1532:0a57` are on the USB bus. It checks each of those ids that is present. A pass does not mean an absent id was verified. CI runs `cargo test` and does not run this scan. GitHub-hosted runners do not have these controllers.

`profiles validate` does not need the controller attached and does not need root. Exit 0 means the catalog loaded. Exit 1 means it did not. The error names the file, and when the profile was parsed, the profile id and the field.

`--profiles PATH` validates or lists a replacement file. The file shipped in the pull request is `profiles/controllers.toml`. Profiles load in file order. Duplicate ids and overlapping selectors are errors. A runtime scan that still matches two profiles reports `ambiguous` instead of picking one. That path is for catalogs built in code. The loader does not accept an overlapping file.

## Pull request

1. Fork and branch from `master`.
2. Add the profile to `profiles/controllers.toml`.
3. Say what you observed: USB id, interface class/subclass/protocol, driver symlink, whether a `js*` or gamepad evdev node existed, and the distribution. Say if you did not test the buttons.
4. Run the checks above.
5. Open a pull request. The CI workflow runs the same checks. It does not publish a build.

## What to leave out

Do not paste hardware serial numbers. Do not run `padcompat --show-serial` for a report you will post. Do not put a serial in `notes`.

Do not include your username, home directory, or hostname. Sysfs names such as `1-7` are fine. `/dev/input/event9` and `/dev/input/js0` are fine.

Strip any path under `/home` before you paste a log. `padcompat list` already omits serials unless `--show-serial` is set. Keep that default.
