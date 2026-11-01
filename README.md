# padcompat

Read-only Linux gamepad discovery. Milestone 1 enumerates controller-like USB and input devices, matches a small declarative profile catalog, and reports hotplug without restarting.

This is not an XInput reimplementation and it does not replace kernel drivers.

## Safety

The program does not need root. Its device I/O is:

- read files under `/sys/bus/usb/devices`
- read `/proc/bus/input/devices` and `/proc/sys/kernel/osrelease`
- `stat` `/dev/input/event*` and `/dev/input/js*` (it does not open them)
- listen on a udev monitor for the `usb` and `input` subsystems

It does not bind or unbind drivers, write sysfs, change udev or systemd, load modules, create virtual gamepads, or change polling rates. Opening an evdev fd is intentionally avoided so Steam and other readers keep the device.

## Setup

Rust is a user toolchain (`rustup`, profile `minimal`). No system packages are required beyond the libudev that systemd already installs.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
cargo test
cargo run -- list
cargo run -- watch
```

`watch` blocks. `--exit-after-ms N` stops after a bounded wait. `--json` prints the same snapshot as JSON, with USB ids and interface bytes as hex strings. `--show-serial` includes the USB serial string; the default text and JSON omit it. `--profiles PATH` replaces the built-in catalog with another version-1 file.

## What a row means

`connection` is the observed attachment. A USB dongle or cable is `usb` even when the evdev node itself is virtual. `input-bus` is the bus of the gamepad evdev node. xone's GIP gamepad is `input-bus: virtual` while `connection` stays `usb`. `form` is the catalog's product link (`wired` or `usb-receiver`), not the kernel bus. `catalog` is the profile name, which stays stable when the USB product string is generic.

`driver` is the USB **interface** driver symlink (`xone-wired`, `xpad-noone`), not the module name (`xone_wired`, `xpad_noone`). `unbound` means that interface has no driver symlink. `status: available` means a gamepad evdev or `js*` node exists. `no-gamepad-input` means the USB device is present without one.

Unknown devices are still listed when they expose a gamepad/joystick node, or an Xbox-style vendor interface (`ff/47/d0`, `ff/5d/01`, `ff/5d/81`). Keyboards, mice, and generic HID devices without those signals are omitted. A HID node whose product string says "Gamepad" is not enough.

## Architecture

```
sysfs USB devices ──┐
                    ├─ assemble ─ snapshot ─ list / json
/proc input devices ┘       │
                            └─ diff ─ watch events
udev monitor (usb, input) ── rescan ── diff
profiles/controllers.toml ── validate ── match
```

Matching key is `(vendor id, product id, interface class/subclass/protocol [, interface number])`. Vendor/product id alone does not select a profile. The loader rejects a profile with no interface selector, a duplicate id, an unknown field, and two profiles that could match the same interface. Several runtime matches become `ambiguous` instead of a silent pick.

There is no plugin trait. The next catalog is another version-1 TOML file, or a later `version` once the struct grows (`bcdDevice`, required interface number, serial). Unknown keys fail validation on purpose.

Hotplug does not trust a single uevent. On add/change/bind it scans at 200ms and again about one second later, because xone creates the GIP input node after the USB interfaces exist. Disconnects scan immediately. The udev socket is non-blocking, so the process polls it.

## Supported hardware

| Profile | USB id | Interface used to match | Expected driver | Form |
| --- | --- | --- | --- | --- |
| `gamesir-g7-se` | `3537:1010` | `ff/47/d0` (observed) | `xone-wired` | wired |
| `gamesir-g7-pro-8k-receiver` | `3537:1033` | `ff/5d/01` (observed) | `xpad-noone` | usb-receiver |
| `razer-wolverine-v3-pro-8k` | `1532:0a57` | `ff/5d/01` (observed) | `xpad-noone` | wired |

`1532:0a57` is the wired Wolverine V3 Pro. The 2.4 GHz dongle is `1532:0a59` and is not in the catalog. The G7 Pro receiver's gamepad interface was observed as `ff/5d/01` on `xpad-noone`. Its USB product string is the generic Xbox 360 name; the catalog name is `GameSir G7 Pro 8K`.

## Limitations

- Bitmap parsing assumes 64-bit kernel `unsigned long` words, most-significant word first, which is what `/proc/bus/input/devices` prints on x86_64.
- Users who are not in the `input` group can still enumerate nodes. The program never reads pad state.
- A connect report can lag the GIP handshake. A later uevent, or the second delayed scan, updates the row.
- Two identical controllers are distinguished by USB topology (`1-6`), which changes when the port changes. The USB serial stays in memory for that correlation and is omitted from text and JSON unless `--show-serial` is set.
- Bluetooth pads with no USB parent show up as unknown input devices. None of the catalog profiles match without an interface descriptor.
- The discovery heuristic for unknown Xbox-style interfaces is fixed in code. New profiles belong in the TOML file.

## Out of scope

Driver rebinding, module loading, udev rules, virtual gamepads, privileged helpers, and a GUI.

## License

Licensed under either of the Apache License, Version 2.0 or the MIT license, at your option. See `LICENSE-APACHE` and `LICENSE-MIT`.
