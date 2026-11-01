## What changed

## Hardware evidence

Which levels were actually observed? Recognized, driver bound, input available, functionally verified. A USB id alone is not functional verification.

- Distribution:
- USB id:
- Interface selectors:
- Driver symlink basename:

Leave this section empty for a change that does not add or edit a controller.

## Checks

- [ ] `cargo run -- profiles validate`
- [ ] `cargo test`
- [ ] No hardware serial, username, home path, or hostname in the diff or the description
