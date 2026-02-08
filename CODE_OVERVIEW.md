# Code Overview

This project implements a simple multi-target ping plotter in Rust. The binary reads a list of IP addresses, pings each one on a fixed cadence, and prints aggregated latency statistics to the console while optionally writing events to a log file.

## Key Behavior
- **Input**: IPs from a text file (default `ips.txt` next to the executable). Each non-empty line is treated as one target.
- **Cadence**: All pings are aligned to even-second boundaries and repeat every 2 seconds.
- **Timeouts**: Each ping process is killed after ~1900 ms to prevent stalls on unreachable targets.
- **Stats**: Per-IP counters for success/total plus min/avg/max latency in milliseconds (parsed from the `ping` output).
- **Logging**:
  - Each cycle that sees failures logs a line with a timestamp and the list of unreachable targets.
  - When the program exits because its optional duration elapsed, it writes the final console table to the log.
- **Defaults**: Without CLI arguments, the app uses `ips.txt` and `result.txt` next to the executable and runs indefinitely.

## Main Components
- `parse_time`: Extracts RTT in milliseconds from platform-specific `ping` stdout (`time=`, `Zeit=`, `time<1ms`, etc.).
- `ping_once`: Invokes the system `ping` with OS-specific arguments and enforces a hard timeout via `wait-timeout`. Captures stdout/stderr to keep the console clean.
- `Stats` struct: Tracks success/total counts and latency aggregates (min, max, sum, sample count).
- Scheduling:
  - Uses `SystemTime` + `Instant` to align the first tick to the next even second.
  - Worker threads sleep until the next tick, run `ping_once`, update shared stats (`Arc<Mutex<HashMap<...>>>`), and advance by 2 seconds.
  - The render loop uses the same cadence to clear and redraw the table.
- Logging helpers:
  - `timestamp` (via `chrono::Local`) for human-readable times.
  - `append_log_line` appends to the log file (created if missing).

## CLI Handling
Arguments are position-flexible:
- First numeric argument → run duration in seconds (optional).
- First non-numeric argument → IP list path (optional; defaults to `ips.txt` next to the binary).
- Next argument (if present) → log file path (optional; defaults to `result.txt` next to the binary).

Examples:
- `ping-plotter` (defaults: `ips.txt`, `result.txt`, infinite runtime)
- `ping-plotter 120` (120 seconds, default paths)
- `ping-plotter /path/ips.txt 120 /path/result.txt`
- `ping-plotter /path/ips.txt /path/result.txt` (no duration)

## Concurrency and Safety
- Shared stats map is protected by a mutex.
- Each IP gets its own worker thread; the render loop is single-threaded.
- Timeouts ensure threads don’t block on slow/unreachable hosts.
- Logging ignores I/O errors to avoid crashing the main loop.

## Platform Notes
- Uses system `ping`:
  - Windows: `ping -n 1 -w 1900`
  - macOS: `ping -c 1 -W 1900`
  - Linux (iputils): `ping -c 1 -W 2`
- RTT parsing may need adjustment for localized `ping` outputs; current patterns cover common English/German strings.

## Extensibility Ideas
- Add CSV/JSON export of per-IP timelines.
- Add a max-thread limit or rate control when the IP list is large.
- Expose a simple HTTP status endpoint for scraping.
- Add unit/integration tests using a mocked `ping` command or `Command` injection.
