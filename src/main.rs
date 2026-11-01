use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use padcompat::{
    ReportOptions, WatchStop, builtin_profiles, format_event, format_event_json, format_snapshot,
    format_snapshot_json, load_profiles, scan_system, watch,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliFailure::Usage(message)) => {
            eprintln!("padcompat: {message}");
            eprintln!("run `padcompat --help` for usage");
            ExitCode::from(2)
        }
        Err(CliFailure::Run(message)) => {
            eprintln!("padcompat: {message}");
            ExitCode::from(1)
        }
    }
}

#[derive(Debug)]
enum CliFailure {
    Usage(String),
    Run(String),
}

#[derive(Debug)]
struct Args {
    command: Command,
    json: bool,
    profiles: Option<std::path::PathBuf>,
    exit_after_ms: Option<u64>,
    show_serial: bool,
    help: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    List,
    Watch,
}

fn run() -> Result<(), CliFailure> {
    let raw: Vec<String> = env::args().skip(1).collect();
    let args = parse_args(&raw)?;
    if args.help {
        print!("{}", help_text());
        return Ok(());
    }
    let profiles = load_catalog(args.profiles.as_deref())?;
    let options = ReportOptions {
        show_serial: args.show_serial,
    };
    match args.command {
        Command::List => {
            let snapshot =
                scan_system(&profiles).map_err(|err| CliFailure::Run(err.to_string()))?;
            if args.json {
                let json = format_snapshot_json(&snapshot, options)
                    .map_err(|err| CliFailure::Run(format!("json: {err}")))?;
                println!("{json}");
            } else {
                print!("{}", format_snapshot(&snapshot, options));
            }
            Ok(())
        }
        Command::Watch => {
            let timeout = args.exit_after_ms.map(Duration::from_millis);
            let json = args.json;
            if json {
                eprintln!("padcompat: watching udev subsystems usb,input");
            }
            let stop = watch(
                &profiles,
                timeout,
                |snapshot| {
                    if json {
                        match format_snapshot_json(snapshot, options) {
                            Ok(text) => println!("{text}"),
                            Err(err) => eprintln!("padcompat: json: {err}"),
                        }
                    } else {
                        print!("{}", format_snapshot(snapshot, options));
                        println!("watching: udev subsystems usb,input");
                    }
                },
                |event| {
                    if json {
                        match format_event_json(event, options) {
                            Ok(text) => println!("{text}"),
                            Err(err) => eprintln!("padcompat: json: {err}"),
                        }
                    } else {
                        println!("{}", format_event(event));
                    }
                },
            )
            .map_err(|err| CliFailure::Run(err.to_string()))?;
            if matches!(stop, WatchStop::Timeout) {
                eprintln!("padcompat: watch stopped after timeout");
            }
            Ok(())
        }
    }
}

fn load_catalog(path: Option<&Path>) -> Result<Vec<padcompat::ControllerProfile>, CliFailure> {
    match path {
        None => builtin_profiles().map_err(|err| CliFailure::Run(err.to_string())),
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|err| CliFailure::Run(format!("read {}: {err}", path.display())))?;
            load_profiles(&text).map_err(|err| CliFailure::Run(err.to_string()))
        }
    }
}

fn parse_args(args: &[String]) -> Result<Args, CliFailure> {
    let mut command = None;
    let mut json = false;
    let mut profiles = None;
    let mut exit_after_ms = None;
    let mut show_serial = false;
    let mut help = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" | "help" => help = true,
            "--json" => json = true,
            "--show-serial" => show_serial = true,
            "--profiles" => {
                let path = iter
                    .next()
                    .ok_or_else(|| CliFailure::Usage("--profiles needs a path".to_string()))?;
                profiles = Some(std::path::PathBuf::from(path));
            }
            value if value.starts_with("--profiles=") => {
                profiles = Some(std::path::PathBuf::from(&value["--profiles=".len()..]));
            }
            "--exit-after-ms" => {
                let value = iter.next().ok_or_else(|| {
                    CliFailure::Usage("--exit-after-ms needs a number".to_string())
                })?;
                exit_after_ms = Some(parse_ms(value)?);
            }
            value if value.starts_with("--exit-after-ms=") => {
                exit_after_ms = Some(parse_ms(&value["--exit-after-ms=".len()..])?);
            }
            "list" => command = Some(set_command(command, Command::List)?),
            "watch" => command = Some(set_command(command, Command::Watch)?),
            other => {
                return Err(CliFailure::Usage(format!("unknown argument {other}")));
            }
        }
    }
    let command = command.unwrap_or(Command::List);
    if exit_after_ms.is_some() && command != Command::Watch {
        return Err(CliFailure::Usage(
            "--exit-after-ms is only valid with watch".to_string(),
        ));
    }
    Ok(Args {
        command,
        json,
        profiles,
        exit_after_ms,
        show_serial,
        help,
    })
}

fn set_command(current: Option<Command>, next: Command) -> Result<Command, CliFailure> {
    if current.is_some() {
        return Err(CliFailure::Usage("only one command is allowed".to_string()));
    }
    Ok(next)
}

fn parse_ms(text: &str) -> Result<u64, CliFailure> {
    text.parse::<u64>()
        .map_err(|_| CliFailure::Usage(format!("invalid milliseconds {text}")))
}

fn help_text() -> &'static str {
    "\
padcompat — read-only Linux gamepad discovery

Usage:
  padcompat [list] [--json] [--show-serial] [--profiles PATH]
  padcompat watch [--json] [--show-serial] [--profiles PATH] [--exit-after-ms MS]

list    Print controllers and exit. This is the default.
watch   Print the current controllers, then connect, disconnect, and update events.

Hardware serial numbers are omitted unless --show-serial is set.
Reads sysfs, /proc/bus/input/devices, and udev events. Does not need root.
Does not bind drivers, write sysfs, open evdev devices, or create virtual pads.
"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn defaults_to_list_and_parses_watch_bounds() {
        let list = parse_args(&[]).unwrap();
        assert_eq!(list.command, Command::List);
        assert!(!list.json);
        assert!(!list.show_serial);

        let watch = parse_args(&args(&[
            "watch",
            "--exit-after-ms",
            "1500",
            "--json",
            "--show-serial",
        ]))
        .unwrap();
        assert_eq!(watch.command, Command::Watch);
        assert_eq!(watch.exit_after_ms, Some(1500));
        assert!(watch.json);
        assert!(watch.show_serial);
        assert!(
            parse_args(&args(&["--profiles=./profiles/controllers.toml"]))
                .unwrap()
                .profiles
                .is_some()
        );
    }

    #[test]
    fn rejects_unknown_flags_and_misplaced_timeout() {
        assert!(matches!(
            parse_args(&args(&["--nope"])),
            Err(CliFailure::Usage(_))
        ));
        assert!(matches!(
            parse_args(&args(&["list", "--exit-after-ms", "10"])),
            Err(CliFailure::Usage(_))
        ));
        assert!(matches!(
            parse_args(&args(&["list", "watch"])),
            Err(CliFailure::Usage(_))
        ));
    }
}
