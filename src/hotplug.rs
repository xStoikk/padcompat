//! udev hotplug loop.
//!
//! `MonitorSocket`'s iterator is non-blocking. [`watch`] polls the socket, then
//! re-reads sysfs. It never writes a udev attribute and never calls the
//! `Device::set_attribute_value` API.

use std::io;
use std::os::fd::AsRawFd;
use std::thread;
use std::time::{Duration, Instant};

use crate::assemble::diff;
use crate::model::{DeviceEvent, Snapshot};
use crate::profile::ControllerProfile;
use crate::scan::scan_system;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchStop {
    Timeout,
}

/// Print-ready callback loop. `timeout` of `None` waits until the process is killed.
///
/// After an add, change, or bind event the scan runs twice (200ms, then ~1s).
/// xone creates the GIP input node after the USB interfaces appear, so the
/// second scan can turn a `no-gamepad-input` report into `available` without
/// another event. Disconnects are scanned immediately.
pub fn watch(
    profiles: &[ControllerProfile],
    timeout: Option<Duration>,
    mut on_snapshot: impl FnMut(&Snapshot),
    mut on_event: impl FnMut(&DeviceEvent),
) -> io::Result<WatchStop> {
    let mut current = scan_system(profiles)?;
    on_snapshot(&current);
    let socket = udev_socket()?;
    let deadline = timeout.map(|duration| Instant::now() + duration);
    let fd = socket.as_raw_fd();

    loop {
        let wait_ms = match deadline {
            None => -1,
            Some(end) => {
                let left = end.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Ok(WatchStop::Timeout);
                }
                i32::try_from(left.as_millis()).unwrap_or(i32::MAX)
            }
        };
        if !poll_fd(fd, wait_ms)? {
            return Ok(WatchStop::Timeout);
        }

        let mut saw_remove = false;
        let mut saw_other = false;
        while let Some(event) = socket.iter().next() {
            match event.event_type() {
                udev::EventType::Remove | udev::EventType::Unbind => saw_remove = true,
                _ => saw_other = true,
            }
        }
        if !saw_remove && !saw_other {
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        if !saw_other {
            publish(&mut current, profiles, &mut on_event)?;
            continue;
        }
        for delay_ms in [200u64, 800] {
            if !sleep_budget(delay_ms, deadline) {
                return Ok(WatchStop::Timeout);
            }
            publish(&mut current, profiles, &mut on_event)?;
        }
    }
}

fn publish(
    current: &mut Snapshot,
    profiles: &[ControllerProfile],
    on_event: &mut impl FnMut(&DeviceEvent),
) -> io::Result<()> {
    let next = scan_system(profiles)?;
    for event in diff(current, &next) {
        on_event(&event);
    }
    *current = next;
    Ok(())
}

fn sleep_budget(delay_ms: u64, deadline: Option<Instant>) -> bool {
    let delay = Duration::from_millis(delay_ms);
    match deadline {
        None => {
            thread::sleep(delay);
            true
        }
        Some(end) => {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            thread::sleep(delay.min(left));
            true
        }
    }
}

fn udev_socket() -> io::Result<udev::MonitorSocket> {
    udev::MonitorBuilder::new()
        .map_err(|err| io::Error::other(format!("create udev monitor: {err}")))?
        .match_subsystem("usb")
        .map_err(|err| io::Error::other(format!("filter usb subsystem: {err}")))?
        .match_subsystem("input")
        .map_err(|err| io::Error::other(format!("filter input subsystem: {err}")))?
        .listen()
        .map_err(|err| io::Error::other(format!("listen for udev events: {err}")))
}

fn poll_fd(fd: std::os::fd::RawFd, timeout_ms: i32) -> io::Result<bool> {
    let mut fds = [libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    }];
    loop {
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };
        if ready < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        return Ok(ready > 0);
    }
}
