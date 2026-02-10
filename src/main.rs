use std::{
    collections::HashMap,
    env,
    fs,
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::Local;
use clap::Parser;
use crossterm::{
    cursor::MoveTo,
    execute,
    terminal::{Clear, ClearType},
};
use wait_timeout::ChildExt;

const INTERVAL: Duration = Duration::from_secs(2);
const PING_TIMEOUT_MS: u64 = 1900;

#[derive(Parser, Debug)]
#[command(name = "ping-plotter")]
#[command(about = "Ping multiple IPs on a fixed interval and display stats", long_about = None)]
struct Args {
    /// Run duration in seconds (omit to run forever)
    #[arg(short = 'd', long = "duration")]
    duration: Option<u64>,

    /// Path to the IP list file
    #[arg(short = 'i', long = "ips")]
    ip_file: Option<PathBuf>,

    /// Path to the log file
    #[arg(short = 'l', long = "log")]
    log_file: Option<PathBuf>,
}

#[derive(Default, Clone, Copy)]
struct Stats {
    success: u64,
    total: u64,
    min_ms: Option<f64>,
    max_ms: Option<f64>,
    sum_ms: f64,
    samples: u64,
}

impl Stats {
    fn record(&mut self, success: bool, latency_ms: Option<f64>) {
        self.total += 1;
        if success {
            self.success += 1;
            if let Some(ms) = latency_ms {
                self.min_ms = Some(self.min_ms.map_or(ms, |cur| cur.min(ms)));
                self.max_ms = Some(self.max_ms.map_or(ms, |cur| cur.max(ms)));
                self.sum_ms += ms;
                self.samples += 1;
            }
        }
    }

    fn avg_ms(&self) -> Option<f64> {
        if self.samples > 0 {
            Some(self.sum_ms / self.samples as f64)
        } else {
            None
        }
    }
}

struct PingResult {
    ip: String,
    success: bool,
    latency_ms: Option<f64>,
}

fn parse_time(stdout: &[u8]) -> Option<f64> {
    // Typical ping outputs: 'time=XX.XXX ms', 'Zeit=XXms', 'time<1ms'
    let text = String::from_utf8_lossy(stdout);
    for part in text.split_whitespace() {
        let lower = part.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("time<").or_else(|| lower.strip_prefix("zeit<")) {
            let value = rest.trim_end_matches("ms");
            if let Ok(ms) = value.parse::<f64>() {
                return Some(ms.min(1.0) / 2.0); // treat <1ms as ~0.5ms
            }
        }
        if let Some(rest) = lower.strip_prefix("time=").or_else(|| lower.strip_prefix("zeit=")) {
            let value = rest.trim_end_matches("ms");
            if value.starts_with('<') {
                return Some(0.5);
            }
            if let Ok(ms) = value.parse::<f64>() {
                return Some(ms);
            }
        }
    }
    None
}

fn timestamp() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn open_log(path: &Path) -> Option<BufWriter<fs::File>> {
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => Some(BufWriter::new(file)),
        Err(err) => {
            eprintln!("Failed to open log file {}: {err}", path.display());
            None
        }
    }
}

fn append_log_line(writer: &mut Option<BufWriter<fs::File>>, line: &str) {
    if let Some(w) = writer.as_mut() {
        if writeln!(w, "{line}").is_err() {
            eprintln!("Failed to write to log file; disabling further logging");
            *writer = None;
        }
    }
}

fn clear_screen() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, Clear(ClearType::All), MoveTo(0, 0));
}

fn ping_once(ip: &str) -> (bool, Option<f64>) {
    // Use system ping to avoid raw socket requirements; capture output to keep console clean.
    let mut cmd = if let Ok(mock) = env::var("PING_PLOTTER_MOCK") {
        let mut c = Command::new(mock);
        c.arg(ip);
        c
    } else {
        let mut c = Command::new("ping");
        if cfg!(target_os = "windows") {
            c.args(["-n", "1", "-w", &PING_TIMEOUT_MS.to_string(), ip]);
        } else if cfg!(target_os = "macos") {
            c.args(["-c", "1", "-W", &PING_TIMEOUT_MS.to_string(), ip]);
        } else {
            let secs = ((PING_TIMEOUT_MS as f64) / 1000.0).ceil().max(1.0) as u64;
            c.args(["-c", "1", "-W", &secs.to_string(), ip]); // iputils uses seconds
        }
        c
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    let timeout = Duration::from_millis(PING_TIMEOUT_MS);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(_) => return (false, None),
    };

    match child.wait_timeout(timeout) {
        Ok(Some(_status)) => match child.wait_with_output() {
            Ok(output) => {
                let success = output.status.success();
                let time_ms = if success { parse_time(&output.stdout) } else { None };
                (success, time_ms)
            }
            Err(_) => (false, None),
        },
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            (false, None)
        }
        Err(_) => (false, None),
    }
}

fn default_paths() -> (PathBuf, PathBuf) {
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let default_ip = exe_dir.join("ips.txt");
    let default_log = exe_dir.join("result.txt");
    (default_ip, default_log)
}

fn align_to_even_second() -> Instant {
    let now_sys = SystemTime::now();
    let now_inst = Instant::now();
    let secs = now_sys
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let next_even_secs = if secs % 2 == 0 { secs + 2 } else { secs + 1 };
    let even_start_sys = UNIX_EPOCH + Duration::from_secs(next_even_secs);
    let delay = even_start_sys
        .duration_since(now_sys)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let first_tick = now_inst + delay;
    first_tick
}

fn spawn_workers(
    ips: &[String],
    tx: mpsc::Sender<PingResult>,
    first_tick: Instant,
    deadline: Option<Instant>,
) -> Vec<thread::JoinHandle<()>> {
    ips.iter()
        .cloned()
        .map(|ip| {
            let tx = tx.clone();
            thread::spawn(move || {
                let mut next_tick = first_tick;
                loop {
                    let now = Instant::now();
                    if let Some(end) = deadline {
                        if now >= end {
                            break;
                        }
                    }
                    if now < next_tick {
                        let sleep_dur = next_tick - now;
                        if let Some(end) = deadline {
                            if now + sleep_dur >= end {
                                thread::sleep(end - now);
                                break;
                            }
                        }
                        thread::sleep(sleep_dur);
                    }
                    if let Some(end) = deadline {
                        if Instant::now() >= end {
                            break;
                        }
                    }
                    let (success, latency_ms) = ping_once(&ip);
                    if tx
                        .send(PingResult {
                            ip: ip.clone(),
                            success,
                            latency_ms,
                        })
                        .is_err()
                    {
                        break;
                    }
                    next_tick += INTERVAL;
                }
            })
        })
        .collect()
}

fn main() {
    let args = Args::parse();
    let (default_ip, default_log) = default_paths();

    let ip_file = args.ip_file.unwrap_or(default_ip.clone());
    let log_path = args.log_file.unwrap_or(default_log.clone());
    let run_for = args.duration.map(Duration::from_secs);

    if !ip_file.exists() {
        eprintln!(
            "IP list file not found: {} (default is ips.txt next to executable)",
            ip_file.display()
        );
        std::process::exit(1);
    }

    let content = fs::read_to_string(&ip_file).unwrap_or_else(|_| {
        eprintln!("Failed to read IP list file: {}", ip_file.display());
        std::process::exit(1);
    });
    let ips: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect();

    if ips.is_empty() {
        eprintln!("No IPs found in {}", ip_file.display());
        std::process::exit(1);
    }

    let first_tick = align_to_even_second();
    let deadline = run_for.map(|d| first_tick + d);

    let (tx, rx) = mpsc::channel::<PingResult>();
    let handles = spawn_workers(&ips, tx, first_tick, deadline);

    let mut stats: HashMap<String, Stats> = HashMap::new();
    let mut prev_counts: HashMap<String, (u64, u64)> = HashMap::new();
    let mut last_display: Vec<String> = Vec::new();
    let mut log_writer = open_log(&log_path);

    let mut next_render = first_tick;
    loop {
        for result in rx.try_iter() {
            let entry = stats.entry(result.ip).or_default();
            entry.record(result.success, result.latency_ms);
        }

        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "{:<20} {:>16} {:>10} {:>10} {:>10}",
            "IP",
            "Erfolg/Gesamt",
            "min (ms)",
            "avg (ms)",
            "max (ms)"
        ));

        let mut unreachable: Vec<String> = Vec::new();
        for ip in &ips {
            let stat = stats.get(ip).copied().unwrap_or_default();
            let fmt = |v: Option<f64>| -> String {
                v.map(|n| format!("{:.2}", n))
                    .unwrap_or_else(|| "-".to_string())
            };
            let count_line = format!(
                "{:<20} {:>16} {:>10} {:>10} {:>10}",
                ip,
                format!("{}/{}", stat.success, stat.total),
                fmt(stat.min_ms),
                fmt(stat.avg_ms()),
                fmt(stat.max_ms),
            );
            lines.push(count_line);

            let prev = prev_counts.get(ip).copied().unwrap_or((0, 0));
            let total_diff = stat.total.saturating_sub(prev.0);
            let success_diff = stat.success.saturating_sub(prev.1);
            if total_diff > 0 && success_diff == 0 {
                unreachable.push(ip.clone());
            }
            prev_counts.insert(ip.clone(), (stat.total, stat.success));
        }

        last_display.clear();
        last_display.extend(lines.iter().cloned());

        clear_screen();
        for line in &lines {
            println!("{line}");
        }

        if !unreachable.is_empty() {
            append_log_line(
                &mut log_writer,
                &format!("[{}] unreachable: {}", timestamp(), unreachable.join(", ")),
            );
        }

        let now = Instant::now();
        if let Some(end) = deadline {
            if now >= end {
                break;
            }
        }
        if now < next_render {
            let sleep_dur = next_render - now;
            if let Some(end) = deadline {
                if now + sleep_dur >= end {
                    thread::sleep(end - now);
                    break;
                }
            }
            thread::sleep(sleep_dur);
        }
        next_render += INTERVAL;
    }

    for result in rx.try_iter() {
        let entry = stats.entry(result.ip).or_default();
        entry.record(result.success, result.latency_ms);
    }

    append_log_line(&mut log_writer, &format!("[{}] Final state:", timestamp()));
    for line in &last_display {
        append_log_line(&mut log_writer, line);
    }

    drop(log_writer);
    for handle in handles {
        let _ = handle.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_common_time_formats() {
        let samples = [
            ("time=12.34 ms", Some(12.34)),
            ("Zeit=56ms", Some(56.0)),
            ("time<1ms", Some(0.5)),
            ("no time here", None),
        ];
        for (input, expected) in samples {
            let got = parse_time(input.as_bytes());
            assert_eq!(got, expected, "failed on input {input}");
        }
    }

    #[cfg(unix)]
    fn make_mock_ping(script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0))
            .as_nanos();
        let unique = format!("mock_ping_{}_{}", std::process::id(), nanos);
        let path = std::env::temp_dir().join(unique);
        fs::write(&path, script).expect("write mock ping");
        let mut perm = fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&path, perm).unwrap();
        path
    }

    #[cfg(unix)]
    fn with_mock<F: FnOnce()>(path: &Path, f: F) {
        let prev = std::env::var("PING_PLOTTER_MOCK").ok();
        unsafe { std::env::set_var("PING_PLOTTER_MOCK", path) };
        f();
        if let Some(val) = prev {
            unsafe { std::env::set_var("PING_PLOTTER_MOCK", val) };
        } else {
            unsafe { std::env::remove_var("PING_PLOTTER_MOCK") };
        }
    }

    #[cfg(unix)]
    #[test]
    fn ping_once_reports_success_and_latency() {
        let script = "#!/bin/sh\necho '64 bytes from 1.1.1.1: time=7.89 ms'\nexit 0\n";
        let path = make_mock_ping(script);
        with_mock(&path, || {
            let (success, latency) = ping_once("1.1.1.1");
            assert!(success);
            assert_eq!(latency, Some(7.89));
        });
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn ping_once_reports_failure() {
        let script = "#!/bin/sh\nexit 1\n";
        let path = make_mock_ping(script);
        with_mock(&path, || {
            let (success, latency) = ping_once("1.1.1.1");
            assert!(!success);
            assert_eq!(latency, None);
        });
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn ping_once_times_out() {
        let script = "#!/bin/sh\nsleep 3\n";
        let path = make_mock_ping(script);
        with_mock(&path, || {
            let start = Instant::now();
            let (success, latency) = ping_once("1.1.1.1");
            let elapsed = start.elapsed();
            assert!(!success);
            assert_eq!(latency, None);
            assert!(
                elapsed < Duration::from_secs(3),
                "expected timeout to cut off sleep, got {:?}",
                elapsed
            );
        });
        let _ = fs::remove_file(path);
    }
}
