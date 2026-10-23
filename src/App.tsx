import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import "./App.css";

const DOCS_URL = "https://opencode.ai/v2/docs/console/models#free-models";

type FreeModel = { id: string; name: string; note: string };
type HistoryEvent = { ts: number; kind: string; message: string; model_ids: string[] };
type Settings = { interval_minutes: number; notify_on_update: boolean };
type State = { settings: Settings; last_checked: number; models: FreeModel[]; history: HistoryEvent[] };
type CheckOutcome = { added: string[]; removed: string[]; updated: string[]; failed: boolean; first_run_baseline: boolean };

const INTERVAL_OPTIONS: { minutes: number; label: string }[] = [
  { minutes: 15, label: "Every 15 minutes" },
  { minutes: 30, label: "Every 30 minutes" },
  { minutes: 60, label: "Every hour" },
  { minutes: 360, label: "Every 6 hours" },
  { minutes: 720, label: "Every 12 hours" },
  { minutes: 1440, label: "Every 24 hours" },
];

function fmtTime(ts: number): string {
  if (!ts) return "never";
  return new Date(ts * 1000).toLocaleString();
}

const KIND_STYLES: Record<string, string> = {
  added: "bg-emerald-500",
  removed: "bg-rose-500",
  updated: "bg-amber-500",
  error: "bg-orange-600",
  baseline: "bg-zinc-500",
};

const TAGS: { key: string; label: string; re: RegExp }[] = [
  { key: "limited", label: "Limited-time", re: /for a limited time/i },
  { key: "no-training", label: "No-data-training", re: /zero-retention|does not use your data/i },
  { key: "feedback", label: "Feedback period", re: /collect feedback/i },
  { key: "stealth", label: "Stealth", re: /stealth/i },
  { key: "region", label: "Region-limited", re: /not available in/i },
];

function tagsFor(note: string): string[] {
  return TAGS.filter((t) => t.re.test(note)).map((t) => t.key);
}

function tagLabel(key: string): string {
  return TAGS.find((t) => t.key === key)?.label ?? key;
}

function Chip({ active, onClick, label, count }: { active: boolean; onClick: () => void; label: string; count: number }) {  return (
    <button
      onClick={onClick}
      className={`rounded-full border px-2.5 py-1 text-[11px] font-medium transition ${
        active
          ? "border-emerald-600 bg-emerald-600/15 text-emerald-400"
          : "border-zinc-800 bg-zinc-900 text-zinc-400 hover:border-zinc-700 hover:text-zinc-300"
      }`}
    >
      {label} <span className={active ? "text-emerald-500/70" : "text-zinc-600"}>{count}</span>
    </button>
  );
}

export default function App() {
  const [state, setState] = useState<State | null>(null);
  const [tab, setTab] = useState<"models" | "history" | "settings">("models");
  const [search, setSearch] = useState("");
  const [tag, setTag] = useState<string>("All");
  const [busy, setBusy] = useState(false);
  const [banner, setBanner] = useState<string | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const [update, setUpdate] = useState<Awaited<ReturnType<typeof check>>>(null);
  const [updatePhase, setUpdatePhase] = useState<"idle" | "downloading" | "done">("idle");
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updateMsg, setUpdateMsg] = useState<string>("");

  const reload = useCallback(async () => {
    const s = await invoke<State>("get_state");
    setState(s);
  }, []);

  const checkAppUpdate = useCallback(async (manual: boolean) => {
    if (manual && updateChecking) return;
    try {
      if (manual) setUpdateChecking(true);
      const u = await check();
      setUpdate(u);
      setUpdateError(null);
      if (manual && !u) setUpdateMsg(`You're on the latest version (${await getVersion()}).`);
      if (u && !manual) setUpdateMsg("");
    } catch (e) {
      setUpdate(null);
      if (manual) setUpdateError(String(e));
    } finally {
      if (manual) setUpdateChecking(false);
    }
  }, [updateChecking]);

  const updateNow = useCallback(async () => {
    if (!update || updatePhase !== "idle") return;
    try {
      setUpdateError(null);
      setUpdatePhase("downloading");
      await update.downloadAndInstall();
      setUpdatePhase("done");
      await relaunch();
    } catch (e) {
      setUpdateError(String(e));
      setUpdatePhase("idle");
    }
  }, [update, updatePhase]);

  useEffect(() => {
    void reload();
    getVersion().then(setAppVersion).catch(() => {});
    void checkAppUpdate(false);
    isEnabled().then(setAutostart).catch(() => setAutostart(false));
    let unlisten: (() => void) | undefined;
    listen("state-changed", () => void reload()).then((fn) => (unlisten = fn));
    return () => {
      unlisten?.();
    };
  }, [reload, checkAppUpdate]);

  async function checkNow() {
    setBusy(true);
    setBanner(null);
    try {
      const outcome = await invoke<CheckOutcome>("check_now");
      if (outcome.failed) {
        setBanner("Check failed — site unreachable or changed. Last good snapshot kept.");
      } else if (outcome.first_run_baseline) {
        setBanner("Baseline saved.");
      } else {
        const parts: string[] = [];
        if (outcome.added.length) parts.push(`+${outcome.added.length} new free`);
        if (outcome.removed.length) parts.push(`-${outcome.removed.length} no longer free`);
        if (outcome.updated.length) parts.push(`${outcome.updated.length} updated`);
        setBanner(parts.length ? parts.join(" · ") : "No changes detected");
      }
      await reload();
    } catch (e) {
      setBanner(`Error: ${e}`);
    } finally {
      setBusy(false);
    }
  }

  async function saveSettings(patch: Partial<Settings>) {
    if (!state) return;
    const newSettings = { ...state.settings, ...patch };
    await invoke("set_settings", { settings: newSettings });
    await reload();
  }

  async function toggleAutostart(on: boolean) {
    setAutostart(on);
    try {
      await (on ? enable() : disable());
    } catch {
      setAutostart(!on);
    }
  }

  const tagCounts: Record<string, number> = {};
  for (const m of state?.models ?? []) {
    for (const t of tagsFor(m.note)) tagCounts[t] = (tagCounts[t] ?? 0) + 1;
  }

  const filtered = (state?.models ?? [])
    .filter((m) => tag === "All" || tagsFor(m.note).includes(tag))
    .filter((m) => `${m.name} ${m.id} ${m.note}`.toLowerCase().includes(search.toLowerCase()));

  return (
    <div className="flex h-full flex-col bg-zinc-950 font-sans text-sm text-zinc-200">
      <header className="flex items-center justify-between border-b border-zinc-800 px-4 py-3">
        <div>
          <div className="font-semibold text-zinc-100">OpenCode Free Models</div>
          <div className="mt-0.5 text-[11px] text-zinc-500">Checked: {fmtTime(state?.last_checked ?? 0)}</div>
        </div>
        <div className="flex items-center gap-2">
          <span className="rounded-full bg-emerald-500/15 px-2 py-0.5 text-[11px] font-medium text-emerald-400">
            {state?.models.length ?? "…"} free
          </span>
          <button
            onClick={checkNow}
            disabled={busy}
            className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-emerald-500 disabled:opacity-50"
          >
            {busy ? "Checking…" : "Check now"}
          </button>
        </div>
      </header>

      {banner && (
        <div className="flex items-center justify-between gap-2 border-b border-zinc-800 bg-emerald-500/10 px-4 py-2 text-xs text-emerald-300">
          <span className="min-w-0 truncate">{banner}</span>
          <button
            onClick={() => setBanner(null)}
            aria-label="Dismiss"
            className="shrink-0 rounded px-1 text-zinc-500 transition hover:text-zinc-200"
          >
            ✕
          </button>
        </div>
      )}

      <nav className="flex border-b border-zinc-800">
        {(["models", "history", "settings"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`flex-1 px-3 py-2 text-xs font-medium capitalize transition ${
              tab === t ? "border-b-2 border-emerald-500 text-emerald-400" : "text-zinc-500 hover:text-zinc-300"
            }`}
          >
            {t === "history" ? "History" : t}
          </button>
        ))}
      </nav>

      <main className="min-h-0 flex-1 overflow-y-auto p-3">
        {tab === "models" && (
          <>
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search models…"
              className="mb-2 w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-2 text-xs text-zinc-200 placeholder-zinc-600 outline-none focus:border-emerald-600"
            />
            <div className="mb-3 flex flex-wrap gap-1.5">
              <Chip active={tag === "All"} onClick={() => setTag("All")} label="All" count={state?.models.length ?? 0} />
              {TAGS.map((t) =>
                tagCounts[t.key] ? (
                  <Chip key={t.key} active={tag === t.key} onClick={() => setTag(t.key)} label={t.label} count={tagCounts[t.key]} />
                ) : null,
              )}
            </div>
            {filtered.length === 0 && state && <p className="p-4 text-center text-xs text-zinc-600">No models match this filter.</p>}
            <ul className="flex flex-col gap-2">
              {filtered.map((m) => (
                <li key={m.id}>
                  <button
                    onClick={() => void openUrl(DOCS_URL)}
                    className="w-full rounded-lg border border-zinc-800 bg-zinc-900 p-3 text-left transition hover:border-emerald-700 hover:bg-zinc-900/70"
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="font-medium text-zinc-100">{m.name}</span>
                      <span className="flex shrink-0 flex-wrap items-center justify-end gap-1">
                        {tagsFor(m.note).map((t) => (
                          <span key={t} className="rounded bg-zinc-800 px-1.5 py-0.5 text-[10px] text-zinc-400">
                            {tagLabel(t)}
                          </span>
                        ))}
                        <span className="rounded bg-emerald-500/15 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-emerald-400">
                          Free
                        </span>
                        <span className="text-zinc-600">↗</span>
                      </span>
                    </div>
                    <div className="mt-0.5 font-mono text-[11px] text-zinc-500">{m.id}</div>
                    {m.note && <p className="mt-1.5 text-[11px] leading-snug text-zinc-400">{m.note}</p>}
                  </button>
                </li>
              ))}
            </ul>
            <button
              onClick={() => void openUrl(DOCS_URL)}
              className="mt-3 w-full rounded-md border border-zinc-800 py-2 text-xs text-zinc-400 transition hover:border-emerald-700 hover:text-emerald-400"
            >
              Open official models page ↗
            </button>
          </>
        )}

        {tab === "history" && (
          <div className="flex flex-col gap-2">
            {state && state.history.length === 0 && (
              <p className="p-4 text-center text-xs text-zinc-600">No changes detected yet.</p>
            )}
            {(state?.history ?? []).map((h, i) => (
              <div key={`${h.ts}-${i}`} className="flex items-start gap-2.5 rounded-lg border border-zinc-800 bg-zinc-900 p-3">
                <span className={`mt-1 size-2 shrink-0 rounded-full ${KIND_STYLES[h.kind] ?? "bg-zinc-500"}`} />
                <div className="min-w-0">
                  <div className="text-xs text-zinc-200">{h.message}</div>
                  <div className="mt-0.5 text-[10px] text-zinc-600">{fmtTime(h.ts)}</div>
                </div>
              </div>
            ))}
          </div>
        )}

        {tab === "settings" && (
          <div className="flex flex-col gap-3">
            {update ? (
              <div className="rounded-lg border border-emerald-800 bg-emerald-500/10 p-3.5">
                <div className="flex items-center justify-between gap-2">
                  <div className="min-w-0">
                    <div className="text-xs font-medium text-emerald-300">Update available</div>
                    <div className="mt-0.5 text-[11px] text-zinc-400">
                      v{update.version} — installing replaces v{appVersion} and restarts the app.
                    </div>
                  </div>
                  <button
                    onClick={updateNow}
                    disabled={updatePhase !== "idle"}
                    className="shrink-0 rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white transition hover:bg-emerald-500 disabled:opacity-50"
                  >
                    {updatePhase === "downloading" ? "Updating…" : "Update & restart"}
                  </button>
                </div>
                {updateError && <p className="mt-2 text-[11px] text-rose-400">{updateError}</p>}
              </div>
            ) : (
              <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-3.5">
                <div className="flex items-center justify-between gap-2">
                  <div className="min-w-0">
                    <div className="text-xs font-medium text-zinc-200">App version</div>
                    <div className="mt-0.5 text-[11px] text-zinc-500">
                      {updateMsg || `v${appVersion} — updates install with one click.`}
                    </div>
                  </div>
                  <button
                    onClick={() => void checkAppUpdate(true)}
                    disabled={updateChecking}
                    className="shrink-0 rounded-md border border-zinc-700 px-3 py-1.5 text-xs text-zinc-300 transition hover:border-emerald-600 hover:text-emerald-400 disabled:opacity-50 disabled:hover:border-zinc-700 disabled:hover:text-zinc-300"
                  >
                    {updateChecking ? "Checking…" : "Check for updates"}
                  </button>
                </div>
                {updateError && <p className="mt-2 text-[11px] text-rose-400">{updateError}</p>}
              </div>
            )}
            <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-3.5">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-xs font-medium text-zinc-200">Start with Windows</div>
                  <div className="mt-0.5 text-[11px] text-zinc-500">Runs in the tray at sign-in.</div>
                </div>
                <Toggle checked={autostart} onChange={toggleAutostart} />
              </div>
            </div>
            <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-3.5">
              <div className="text-xs font-medium text-zinc-200">Check interval</div>
              <div className="mt-0.5 mb-2 text-[11px] text-zinc-500">How often to check the docs page.</div>
              <select
                value={state?.settings.interval_minutes ?? 60}
                onChange={(e) => void saveSettings({ interval_minutes: Number(e.target.value) })}
                className="w-full rounded-md border border-zinc-800 bg-zinc-950 px-3 py-2 text-xs text-zinc-200 outline-none focus:border-emerald-600"
              >
                {INTERVAL_OPTIONS.map((o) => (
                  <option key={o.minutes} value={o.minutes}>
                    {o.label}
                  </option>
                ))}
              </select>
            </div>
            <div className="rounded-lg border border-zinc-800 bg-zinc-900 p-3.5">
              <div className="flex items-center justify-between">
                <div>
                  <div className="text-xs font-medium text-zinc-200">Notify on updates</div>
                  <div className="mt-0.5 text-[11px] text-zinc-500">Also toast when a free model's notes change.</div>
                </div>
                <Toggle
                  checked={state?.settings.notify_on_update ?? true}
                  onChange={(v) => void saveSettings({ notify_on_update: v })}
                />
              </div>
            </div>
            <p className="px-1 text-center text-[10px] text-zinc-600">State saved locally · no API key needed</p>
          </div>
        )}
      </main>
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      onClick={() => onChange(!checked)}
      className={`relative h-5 w-9 shrink-0 rounded-full transition ${checked ? "bg-emerald-600" : "bg-zinc-700"}`}
    >
      <span
        className={`absolute top-0.5 size-4 rounded-full bg-white transition-all ${checked ? "left-[18px]" : "left-0.5"}`}
      />
    </button>
  );
}
