# padcompat

padcompat is a Linux gamepad compatibility runtime. The long-term aim is a small service that notices a controller, names the driver situation, and applies a community profile so the pad is usable without a per-device setup ritual.

That service is not what this repository runs today.

The program in this tree is a read-only command-line tool. It looks at USB and input devices that Linux has already enumerated, matches them to a TOML catalog, and prints what it found. It does not install drivers, replace XInput, or create a virtual pad.

## Current status

Implemented and used on one development machine:

- Read-only USB discovery.
- Association with evdev and joystick nodes.
- Identification of the bound USB interface driver.
- Matching against `profiles/controllers.toml`.
- Catalog validation, including duplicate and overlap checks.
- udev monitoring for connect and disconnect.
- Text and JSON reports.
- Hardware serials omitted unless `--show-serial` is set.

Not implemented:

- Automatic driver installation.
- Kernel driver management.
- Driver conflict remediation.
- A background service, or installing one.
- Virtual controller emulation.
- An XInput API.
- A GUI.
- Downloading profiles from the network.

A catalog entry does not add kernel protocol support. If Linux has no driver for the device, a TOML row can only name it. padcompat will not load a module or bind an interface.

The catalog is not a claim that every Linux distribution can use every listed pad.

## Verified development controllers

These three devices were checked on one CachyOS machine (kernel `7.3.1-1-cachyos`). The `xpad-noone` driver there is a customized out-of-tree build. padcompat does not ship that driver, does not install it, and does not recommend copying that setup onto another machine.

`expected_driver` is the interface-driver name observed when the profile was written (the basename of the sysfs `driver` symlink). It is not a promise that the same driver exists, is safe, or is sufficient on another distribution.

| Controller | USB id | Match interface | Observed driver | Form |
| --- | --- | --- | --- | --- |
| GameSir G7 SE | `3537:1010` | `ff/47/d0` | `xone-wired` | wired |
| GameSir G7 Pro 8K receiver | `3537:1033` | `ff/5d/01` | `xpad-noone` | usb-receiver |
| Razer Wolverine V3 Pro 8K, wired | `1532:0a57` | `ff/5d/01` | `xpad-noone` | wired |

`1532:0a59` is the Wolverine 2.4 GHz dongle. It was not tested and it is not in the catalog. No other wireless variant was added from a datasheet.

The G7 Pro receiver's USB product string is the generic Xbox 360 name. The catalog name is `GameSir G7 Pro 8K`.

## What a report row means

`connection` is how the kernel attached the device. A cable and a USB dongle are both `usb`. `input-bus` is the bus of the gamepad evdev node. xone's GIP gamepad is `input-bus: virtual` while `connection` stays `usb`. `form` is the catalog's product link (`wired`, `usb-receiver`, `bluetooth`, or `unknown`), not the kernel bus.

`driver` is the USB interface driver (`xone-wired`, `xpad-noone`), not the module name (`xone_wired`, `xpad_noone`). `unbound` means that interface has no driver symlink. `status: available` means a gamepad evdev node or a `js*` node exists. `no-gamepad-input` means the USB device is present without one.

Those are separate from someone actually pressing the buttons. The catalog does not record hands-on testing. See `CONTRIBUTING.md`.

Unknown devices are still listed when they expose a gamepad or joystick node, or an Xbox-style vendor interface (`ff/47/d0`, `ff/5d/01`, `ff/5d/81`). Keyboards, mice, and generic HID devices without those signals are omitted.

## Requirements

Tested only on CachyOS, x86_64, with rustc 1.99.1 and libudev 262. `Cargo.toml` asks for Rust 1.85. This tree was not built on other distributions.

Build time needs a Rust toolchain and pkg-config plus libudev headers. On the development machine those headers were already present at `/usr/lib/pkgconfig/libudev.pc`. Nothing was installed into the system for padcompat.

The GitHub Actions workflow asks Ubuntu's package manager for `libudev-dev` and `pkg-config`. That job has not been observed running on GitHub. Treat those package names as the workflow's request, not as a tested desktop install.

## Build and run

After the repository is published:

```sh
git clone https://github.com/xStoikk/padcompat.git
cd padcompat
```

Rustup, as used on the development machine:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --component rustfmt --component clippy
. "$HOME/.cargo/env"
```

`rustfmt` and `clippy` are for the checks below. `cargo run` does not need them.

```sh
cargo build
cargo run -- list
cargo run -- watch
cargo run -- profiles validate
cargo test
```

`cargo test` does not require a controller and does not need root. The hardware scan is separate:

```sh
cargo test --test live_scan -- --ignored
```

That command fails if none of `3537:1010`, `3537:1033`, or `1532:0a57` are on the USB bus. It checks each of those ids that is actually present. An absent id was not verified.

There is no install script and no systemd unit.

## CLI

```text
padcompat [list] [--json] [--show-serial] [--profiles PATH]
padcompat watch [--json] [--show-serial] [--profiles PATH] [--exit-after-ms MS]
padcompat profiles validate [--profiles PATH]
```

`list` is the default. It prints the current controllers and exits.

`watch` prints that snapshot, then connect, disconnect, and update lines. It blocks until the process is stopped. `--exit-after-ms` is only valid with `watch`.

`profiles validate` checks the built-in catalog, or the file given to `--profiles`. It does not scan hardware. Exit 0 means the file loaded. Exit 1 means it did not. A usage error exits 2.

`--json` prints the same snapshot as JSON. USB ids and interface bytes are hex strings. `--profiles PATH` replaces the built-in catalog for that run. `--show-serial` prints USB serial strings. Leave it off for anything you might paste.

The program does not need root. It reads `/sys/bus/usb/devices`, `/proc/bus/input/devices`, and `/proc/sys/kernel/osrelease`, stats `/dev/input/event*` and `/dev/input/js*` without opening them, and listens for udev events on the `usb` and `input` subsystems. It does not bind drivers, write sysfs, change udev or systemd, load modules, or create virtual pads.

## Profiles

The catalog is `profiles/controllers.toml`, schema version 1. A profile matches only when vendor id, product id, and at least one interface selector match. Vendor and product id alone are not enough. The loader rejects a missing selector, a duplicate id, an unknown field, a driver name that is not a single sysfs basename, and two profiles that could match the same interface. Profile text is data. It is never executed.

`--profiles PATH` loads a replacement file. It does not merge with the built-in catalog.

How to add a controller is in `CONTRIBUTING.md`.

## Tests and CI

`cargo test` runs the unit tests, the catalog CLI tests, and the fixture scan. It skips `tests/live_scan.rs`.

`.github/workflows/ci.yml` is set up to run, on pull requests and on pushes to `master`: formatting, `cargo test`, Clippy with warnings denied, and `profiles validate`. The workflow token is read-only. It does not deploy or publish. That workflow has not run on GitHub yet.

## Limitations

- Bitmap parsing assumes 64-bit `/proc/bus/input/devices` words, most-significant word first.
- You do not need to be in the `input` group to enumerate nodes. The program never reads button state.
- A connect report can lag the xone GIP handshake. A later uevent, or the second delayed scan, updates the row.
- Two identical controllers are distinguished by USB topology (`1-6`). That name changes when the port changes. The serial stays in memory and is left out of text and JSON unless `--show-serial` is set.
- Bluetooth pads with no USB parent show up as unknown input devices. Catalog profiles match USB interface descriptors.
- The heuristic that lists unknown Xbox-style interfaces is fixed in code. New names belong in the TOML file.

## Later work

Not in this tree, and not started here:

- A user service that starts with the session.
- Installing or selecting kernel drivers.
- Detecting and repairing driver conflicts.
- A virtual gamepad.
- An XInput-compatible interface.
- A GUI.
- Fetching catalog updates from the network.

## Contributing

See `CONTRIBUTING.md`. Do not send hardware serials, home-directory paths, usernames, or hostnames.

## License

Licensed under either of the Apache License, Version 2.0 or the MIT license, at your option. See `LICENSE-APACHE` and `LICENSE-MIT`.
