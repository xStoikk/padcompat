# Security

padcompat reads sysfs, proc, and udev. It does not open evdev devices, write sysfs, or run anything stored in a controller profile. `expected_driver` is a name compared to a symlink. It is not a command.

GitHub private vulnerability reporting is a repository setting. It is not on just because the project is public. After the repository exists on GitHub, turn that setting on, then use it. Until then, do not open a public issue for a vulnerability.

Do not include hardware serial numbers, usernames, home-directory paths, or hostnames in the report. Default `padcompat list` output already omits serials. Do not add `--show-serial` to a report.
