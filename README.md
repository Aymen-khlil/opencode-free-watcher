# OpenCode Free Models Watcher

A tiny Windows tray widget (2.25 MB) that keeps an eye on the
[OpenCode Console free models](https://opencode.ai/v2/docs/console/models#free-models) page and
notifies you the moment something changes.

- Checks the official models page **every hour** (configurable) in the background
- Sends a **native Windows toast** when a free model is **added**, **removed**, or its details change
- Click the tray icon to open a compact widget with the current **free model list** (searchable,
  with notes from the docs), a **change history**, and **settings**
- One-click link to the official models page — no account, API key, or backend needed

## Download & install (easiest)

1. Go to [**Releases**](../../releases)
2. Download `opencode-free-watcher_x64-setup.exe` from the newest release
3. Run the installer — done. Find the widget in your **system tray** (double-click it to open)

On first launch it saves a baseline quietly. From then on you get a toast like
*"New free model available: Big Pickle"* whenever OpenCode rotates the free-model list.

## Run from source

```bash
# prerequisites: Node.js LTS + Rust (stable-msvc) + VS Build Tools (C++ workload) + WebView2
git clone https://github.com/Aymen-khlil/opencode-free-watcher.git
cd opencode-free-watcher
npm install
npm run tauri dev      # run in development
npm run tauri build    # produce installer in src-tauri/target/release/bundle/nsis/
```

## How it works

The Rust core fetches `https://opencode.ai/v2/docs/console/models`, parses the pricing table
(rows where input **and** output are `Free` — this also catches stealth models like `big-pickle`
that have no `-free` suffix) plus the per-model notes in the `#free-models` section.
It diffs the result against a local snapshot in

```
%APPDATA%\com.ceazi.opencode-free-watcher\state.json
```

and only notifies on real changes. If the site is unreachable, the last good snapshot is kept —
a failed fetch is never mistaken for "everything was removed".

## Project layout

| Path | What it is |
|---|---|
| `src/` | React + TypeScript + Tailwind widget UI |
| `src/App.tsx` | Widget window (Models / History / Settings tabs) |
| `src-tauri/src/lib.rs` | Tray icon, hourly timer, notifications, Tauri commands |
| `src-tauri/src/monitor.rs` | Fetch + HTML parsing + diff engine |
| `src-tauri/src/store.rs` | Snapshot / history persistence |

## Tech

[Tauri 2](https://tauri.app) (tiny native binary, not Electron) · React 18 · TypeScript ·
Tailwind CSS · `tauri-plugin-notification` (native toasts) · `tauri-plugin-autostart`
(start with Windows toggle in Settings).

## License

[MIT](./LICENSE)
