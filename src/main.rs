use std::{
    collections::HashMap,
    env,
    fs,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    cursor::MoveTo,
    execute,
    terminal::{Clear, ClearType},
};
use chrono::Local;
use wait_timeout::ChildExt;

#[derive(Default, Clone, Copy)]
struct Stats {
    success: u64,
    total: u64,
    min_ms: Option<f64>,
    max_ms: Option<f64>,
    sum_ms: f64,
    samples: u64,
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

fn append_log_line(path: &Path, line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
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
            c.args(["-n", "1", "-w", "1900", ip]);
        } else if cfg!(target_os = "macos") {
            c.args(["-c", "1", "-W", "1900", ip]);
        } else {
            c.args(["-c", "1", "-W", "2", ip]); // iputils uses seconds; 2s approximates 1900ms
        }
        c
    };
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());

    let timeout = Duration::from_millis(1900);
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

fn main() {
    let args: Vec<String> = env::args().collect();
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let default_ip = exe_dir.join("ips.txt");
    let default_log = exe_dir.join("result.txt");

    let mut ip_file = default_ip.clone();
    let mut run_for: Option<Duration> = None;
    let mut log_path: PathBuf = default_log.clone();

    for arg in args.iter().skip(1) {
        if run_for.is_none() {
            if let Ok(secs) = arg.parse::<u64>() {
                run_for = Some(Duration::from_secs(secs));
                continue;
            }
        }
        if ip_file == default_ip {
            ip_file = PathBuf::from(arg);
            continue;
        }
        if log_path == default_log {
            log_path = PathBuf::from(arg);
        }
    }

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

    let stats: Arc<Mutex<HashMap<String, Stats>>> = Arc::new(Mutex::new(HashMap::new()));

    // Align to the next even second boundary to fire all pings in sync.
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
    let deadline = run_for.map(|d| first_tick + d);

    let mut prev_counts: HashMap<String, (u64, u64)> = HashMap::new();
    let mut last_display: Vec<String> = Vec::new();

    for ip in ips.clone() {
        let stats_handle = Arc::clone(&stats);
        let thread_deadline = deadline;
        thread::spawn(move || {
            let mut next_tick = first_tick;
            loop {
                let now = Instant::now();
                if let Some(end) = thread_deadline {
                    if now >= end {
                        break;
                    }
                }
                if now < next_tick {
                    let sleep_dur = next_tick - now;
                    if let Some(end) = thread_deadline {
                        if now + sleep_dur >= end {
                            thread::sleep(end - now);
                            break;
                        }
                    }
                    thread::sleep(sleep_dur);
                }
                if let Some(end) = thread_deadline {
                    if Instant::now() >= end {
                        break;
                    }
                }
                let (success, latency) = ping_once(&ip);
                {
                    let mut map = stats_handle.lock().expect("stats mutex poisoned");
                    let entry = map.entry(ip.clone()).or_default();
                    entry.total += 1;
                    if success {
                        entry.success += 1;
                        if let Some(ms) = latency {
                            entry.min_ms = Some(entry.min_ms.map_or(ms, |cur| cur.min(ms)));
                            entry.max_ms = Some(entry.max_ms.map_or(ms, |cur| cur.max(ms)));
                            entry.sum_ms += ms;
                            entry.samples += 1;
                        }
                    }
                }
                next_tick += Duration::from_secs(2);
            }
        });
    }

    let mut next_render = first_tick;
    loop {
        {
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
            let map = stats.lock().expect("stats mutex poisoned");
            for ip in &ips {
                let stat = map.get(ip).copied().unwrap_or_default();
                let avg_ms = if stat.samples > 0 {
                    Some(stat.sum_ms / stat.samples as f64)
                } else {
                    None
                };
                let fmt = |v: Option<f64>| -> String {
                    v.map(|n| format!("{:.2}", n)).unwrap_or_else(|| "-".to_string())
                };
                let count_line = format!(
                    "{:<20} {:>16} {:>10} {:>10} {:>10}",
                    ip,
                    format!("{}/{}", stat.success, stat.total),
                    fmt(stat.min_ms),
                    fmt(avg_ms),
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

            clear_screen(); // clear screen and reset cursor (cross-platform)
            for line in &lines {
                println!("{line}");
            }

            if !unreachable.is_empty() {
                append_log_line(
                    &log_path,
                    &format!("[{}] unreachable: {}", timestamp(), unreachable.join(", ")),
                );
            }
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
        next_render += Duration::from_secs(2);
    }

    append_log_line(&log_path, &format!("[{}] Final state:", timestamp()));
    for line in &last_display {
        append_log_line(&log_path, line);
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
