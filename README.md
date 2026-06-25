# OST

OST is a colorful terminal UI for exploring local disks, files, network connections, processes, CPU, memory, services, logs, and system metadata. It starts with a visual dashboard for a quick system overview.

## Run

```bash
cargo run
```

## Controls

- `Up`/`Down`: move selection by 1 row
- `Shift+Up`/`Shift+Down`: move selection by 5 rows
- `PageUp`/`PageDown`: move selection by a page
- The app starts with the top tabs focused
- `Up` on the first row: focus the top tabs
- `Left`/`Right` on the top tabs: choose a view
- `Down` or `Enter` on the top tabs: return to the table
- `Enter`/`Right`: enter a directory or open the action menu
- `Left`/`Esc`: go back, close a menu, or clear search
- `/` or `s`: search by keyword; multiple words are matched together
- `Tab`, `Shift+Tab`: switch views
- `1`-`9`: jump to Dashboard, Disk, Network, Process, CPU, Memory, Services, Logs, or System
- `r`: refresh immediately
- `q`: quit

## Actions

- Disk volumes: enter, copy details, copy path
- Files and folders: enter folders, copy details, copy path, create folder, rename, copy to path, move to path, delete with confirmation
- Network rows: copy details, copy port, kill the owning process when PID is available
- Processes: copy details, kill with confirmation
- Services: copy details, copy PID, kill the process when PID is available
- Dashboard, CPU, Memory, Logs, and System rows: copy details

## Views

- `Dashboard`: cyan/green/blue gauges and horizontal percentage bars for CPU, memory, swap, disk usage, top processes, and overall counts
- `Disk`: volumes and interactive directory exploration
- `Network`: current network sockets from `lsof` with PID support, falling back to `netstat`
- `Process`: running processes sorted by CPU
- `CPU`: global and per-core usage
- `Memory`: memory and swap summary
- `Services`: `launchctl` on macOS or `systemctl` on Linux
- `Logs`: recent logs from `log show` on macOS or `journalctl` on Linux
- `System`: OS, kernel, host, uptime, and boot time

## Notes

- Disk and process data come from `sysinfo`.
- Network rows prefer `lsof -nP -iTCP -iUDP` so selected sockets can expose a PID for process actions; `netstat -anv` is used as a fallback.
- Services and logs are best-effort read-only views and depend on host permissions.
- Clipboard copy uses `pbcopy` on macOS, then falls back to an internal buffer if the system clipboard command is unavailable.
