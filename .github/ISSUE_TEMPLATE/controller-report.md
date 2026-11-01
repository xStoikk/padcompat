---
name: Controller report
about: Ask for a controller to be recognized from descriptors you actually read
title: "controller: "
---

Do not include a hardware serial, a username, a home-directory path, or a hostname. Do not paste `padcompat --show-serial` output.

A USB id alone does not mean the pad works.

## Device

- Distribution:
- USB id (`idVendor:idProduct`):
- Interface class / subclass / protocol (hex), and the interface number:
- Bound driver (sysfs `driver` symlink basename, or `unbound`):

## What you actually observed

Check only the levels you verified.

- [ ] Recognized: vendor, product, and an interface selector match a profile, or you are proposing one.
- [ ] Driver bound: Linux attached the driver named above.
- [ ] Input available: a gamepad evdev node or `js*` node exists.
- [ ] Functionally verified: the controls were used, not only enumerated.

If you did not press the controls, say that.

## Catalog note

Say whether this should be a new `profiles/controllers.toml` row. Do not propose an id you have not plugged in.
