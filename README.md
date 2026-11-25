# TexTree

Fast, minimalist, flat-dark disk space analyzer built in Rust with **GPUI-Kit**.

## Features

- **Entire PC & Per-Drive Scanning**: Inspect any individual drive, custom folder, or scan all fixed drives at once under a unified **This PC** tree.
- **Virtualized Tree-Table**: Smooth 60–120 FPS browsing across hundreds of thousands of files and folders, showing `% of Parent`, `Total Size`, `Allocated` (size on disk), item counts, modification timestamps, and status badges.
- **Locked & Inaccessible Inspection**:
  - Counts and highlights `Access Denied` paths during scans without aborting.
  - Uses the Windows **Restart Manager** API in the Inspector to detect which active process (and PID) holds a lock on a selected file.
- **Top Files & Extensions Views**: Instantly find the largest files on disk, group usage by extension, and jump directly to any file in the Tree View.
- **Safe Cleanup**: Move unwanted files or folders to the system Recycle Bin behind an explicit confirmation modal, updating sizes immediately.
- **Obsidian Dark UI**: Strict flat dark palette (`#09090B` / `#111113` / `#18181B` / `#27272A`) with selectable accent presets (**Obsidian Emerald**, **Obsidian Violet**, **Obsidian Monochrome**) in Settings.
- **Built-in Auto-Updater**: Checks GitHub Releases automatically and downloads, verifies (SHA-256), installs, and restarts with one click.

## Build from Source

```bash
cargo test
cargo build --release
```

## Release Workflow

- Pull requests targeting `main` run the cross-platform CI test matrix (`.github/workflows/ci.yml`).
- Merging to `main` automatically resolves the next SemVer tag, builds release installers and archives, generates `SHA256SUMS.txt`, and publishes a GitHub Release (`.github/workflows/release.yml`).
