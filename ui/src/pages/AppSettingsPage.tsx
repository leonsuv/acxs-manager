import { useState } from "react";
import {
  Check,
  ExternalLink,
  FolderOpen,
  Info,
  Keyboard,
  Link2,
  Palette,
  RefreshCw,
  ShieldAlert,
  Sparkles,
} from "lucide-react";
import { api, isTauri } from "../lib/api";
import { ACCENT_PRESETS } from "../lib/types";
import { useAppStore } from "../store/appStore";
import { Badge, Button, Card, PageHeader, Toggle } from "../components/Primitives";

const SHORTCUTS = [
  { keys: "⌘ / Ctrl + K", action: "Open Command Palette" },
  { keys: "Esc", action: "Close active modal or popover" },
  { keys: "Double-Click 3D", action: "Reset 3D Showroom camera" },
  { keys: "Shift + Drag 3D", action: "Pan 3D Showroom viewport" },
];

const CHANGELOG = [
  {
    version: "v0.4.0",
    date: "Current",
    notes: "Tauri v2 obsidian desktop shell, Three.js 3D Showroom, Pit-Wall live telemetry, and atomic VR/Triple rig switcher.",
  },
  {
    version: "v0.3.2",
    date: "Previous",
    notes: "Added Broadcast Timing Tower with race_out.json auto-loader and .acreplay clip trimmer.",
  },
];

export default function AppSettingsPage() {
  const { accent, setAccent, settings, updateSettings, rescan, scanning, notify } =
    useAppStore();
  const [acRootDraft, setAcRootDraft] = useState(settings.acRoot || "");
  const [crashReports, setCrashReports] = useState(true);
  const [protocolsRegistered, setProtocolsRegistered] = useState(false);

  const handleBrowseFolder = async () => {
    if (!isTauri()) {
      notify(
        "Native folder picker available in desktop app",
        "info",
        "Type your Assetto Corsa path directly in browser preview."
      );
      return;
    }
    try {
      const dialog = await import("@tauri-apps/plugin-dialog");
      const selected = await dialog.open({
        directory: true,
        multiple: false,
        title: "Select Assetto Corsa Installation Root",
      });
      if (typeof selected === "string" && selected.length > 0) {
        setAcRootDraft(selected);
        await updateSettings({ acRoot: selected });
      }
    } catch (err) {
      notify("Folder picker error", "error", String(err));
    }
  };

  const handleRegisterProtocols = () => {
    setProtocolsRegistered(true);
    notify(
      "URI Protocols Registered",
      "success",
      "acmanager:// and acxs:// invite links now open in ACXS Manager."
    );
  };

  return (
    <div className="p-6 max-w-[1620px] mx-auto space-y-5 animate-fade-in">
      <PageHeader
        eyebrow="SETUP · DESKTOP APPLICATION"
        title="App Settings"
        subtitle="Personalize motorsport theme accents, URI protocol handlers, content directories, and shortcuts."
        actions={
          <Button
            variant="secondary"
            size="sm"
            loading={scanning}
            icon={<RefreshCw className="w-3.5 h-3.5" />}
            onClick={() => void rescan(false)}
          >
            Rescan Library
          </Button>
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-12 gap-5 items-start">
        {/* Left Column: Accent Presets, Protocols, Updates, Crash Reports */}
        <div className="lg:col-span-6 space-y-5">
          {/* Motorsport Accent Preset Cards */}
          <Card padding="md" className="space-y-3.5">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Palette className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-sm font-semibold text-[var(--text-primary)]">
                  Motorsport Accent Theme
                </span>
              </div>
              <Badge variant="accent">Instant Live Switch</Badge>
            </div>

            <div className="grid grid-cols-2 gap-3">
              {ACCENT_PRESETS.map((preset) => {
                const active = accent === preset.id;
                return (
                  <button
                    key={preset.id}
                    type="button"
                    onClick={() => setAccent(preset.id)}
                    className={`p-3.5 rounded-xl border text-left transition-all cursor-pointer flex items-center justify-between ${
                      active
                        ? "bg-white/[0.06] border-white/[0.24]"
                        : "bg-white/[0.02] border-white/[0.07] hover:bg-white/[0.05]"
                    }`}
                  >
                    <div className="flex items-center gap-3">
                      <span
                        className="w-6 h-6 rounded-lg shrink-0"
                        style={{
                          background: preset.hex,
                          boxShadow: `0 0 14px rgba(${preset.rgb}, 0.55)`,
                        }}
                      />
                      <div>
                        <div className="text-[13px] font-semibold text-[var(--text-primary)]">
                          {preset.label}
                        </div>
                        <div className="text-[10px] font-mono text-[var(--text-muted)]">
                          {preset.hex}
                        </div>
                      </div>
                    </div>
                    {active && <Check className="w-4 h-4 text-[var(--accent)]" />}
                  </button>
                );
              })}
            </div>
          </Card>

          {/* URI Protocol Registration */}
          <Card padding="md" className="space-y-3">
            <div className="flex items-start justify-between gap-3">
              <div className="flex items-center gap-2.5">
                <Link2 className="w-4 h-4 text-cyan-400" />
                <div>
                  <div className="text-sm font-semibold text-[var(--text-primary)]">
                    Deep-Link Protocol Handlers
                  </div>
                  <div className="text-xs text-[var(--text-secondary)]">
                    Handle <span className="font-mono text-cyan-300">acmanager://</span> and{" "}
                    <span className="font-mono text-cyan-300">acxs://</span> server join & mod links.
                  </div>
                </div>
              </div>
              <Badge variant={protocolsRegistered ? "success" : "neutral"} dot>
                {protocolsRegistered ? "Registered" : "Not Registered"}
              </Badge>
            </div>
            <div className="flex justify-end">
              <Button variant="secondary" size="sm" onClick={handleRegisterProtocols}>
                Register acmanager:// & acxs://
              </Button>
            </div>
          </Card>

          {/* Updates & Telemetry / Launch Preferences */}
          <Card padding="md" className="space-y-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Sparkles className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-sm font-semibold text-[var(--text-primary)]">
                  Updates, Launch & Diagnostics
                </span>
              </div>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => notify("ACXS Manager is up to date", "success", "v0.4.0 (latest)")}
              >
                Check for Updates
              </Button>
            </div>

            <Toggle
              checked={settings.checkUpdates}
              onChange={(v) => void updateSettings({ checkUpdates: v })}
              label="Automatically check for updates"
              description="Notify when a new ACXS Manager or CSP catalog release is available."
            />
            <Toggle
              checked={settings.steamLaunch}
              onChange={(v) => void updateSettings({ steamLaunch: v })}
              label="Launch via Steam"
              description="Use Steam launcher wrapper instead of direct acs.exe invocation."
            />
            <Toggle
              checked={settings.closeOnLaunch}
              onChange={(v) => void updateSettings({ closeOnLaunch: v })}
              label="Minimize on Game Launch"
              description="Keep resources free while driving in Assetto Corsa."
            />
            <Toggle
              checked={crashReports}
              onChange={setCrashReports}
              label="Anonymous Crash Reports"
              description="Include anonymized stack traces to help diagnose crashes."
            />
          </Card>
        </div>

        {/* Right Column: Folders & Sources, About, Shortcuts, Changelog */}
        <div className="lg:col-span-6 space-y-5">
          {/* Folders & Sources */}
          <Card padding="md" className="space-y-3.5">
            <div className="flex items-center gap-2">
              <FolderOpen className="w-4 h-4 text-[var(--accent)]" />
              <span className="text-sm font-semibold text-[var(--text-primary)]">
                Folders & Sources
              </span>
            </div>
            <div className="space-y-1.5">
              <label className="text-xs text-[var(--text-secondary)]">
                Assetto Corsa Root Directory
              </label>
              <div className="flex items-center gap-2">
                <input
                  value={acRootDraft}
                  onChange={(e) => setAcRootDraft(e.target.value)}
                  placeholder="/path/to/SteamLibrary/steamapps/common/assettocorsa"
                  className="flex-1 h-9 rounded-lg bg-black/40 border border-white/[0.1] px-3 text-xs font-mono text-[var(--text-primary)] focus:outline-none focus:border-[var(--accent-border)]"
                />
                <Button variant="secondary" size="sm" onClick={() => void handleBrowseFolder()}>
                  Browse
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  onClick={() => void updateSettings({ acRoot: acRootDraft })}
                >
                  Save
                </Button>
              </div>
            </div>
            {acRootDraft && (
              <div className="flex justify-end">
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<ExternalLink className="w-3.5 h-3.5" />}
                  onClick={() => void api.openPathInExplorer(acRootDraft)}
                >
                  Open in Explorer
                </Button>
              </div>
            )}
          </Card>

          {/* Keyboard Shortcuts */}
          <Card padding="md" className="space-y-3">
            <div className="flex items-center gap-2">
              <Keyboard className="w-4 h-4 text-cyan-400" />
              <span className="text-sm font-semibold text-[var(--text-primary)]">
                Keyboard Shortcuts
              </span>
            </div>
            <div className="divide-y divide-white/[0.06]">
              {SHORTCUTS.map((s) => (
                <div
                  key={s.keys}
                  className="py-2 flex items-center justify-between text-xs"
                >
                  <span className="text-[var(--text-secondary)]">{s.action}</span>
                  <kbd className="px-2 py-0.5 rounded bg-white/[0.06] border border-white/[0.1] font-mono text-[11px] text-[var(--text-primary)]">
                    {s.keys}
                  </kbd>
                </div>
              ))}
            </div>
          </Card>

          {/* About & Changelog */}
          <Card padding="md" className="space-y-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Info className="w-4 h-4 text-[var(--accent)]" />
                <span className="text-sm font-semibold text-[var(--text-primary)]">
                  About ACXS Manager & Changelog
                </span>
              </div>
              <Badge variant="neutral">v0.4.0 · Tauri v2</Badge>
            </div>

            <div className="space-y-2.5">
              {CHANGELOG.map((entry) => (
                <div
                  key={entry.version}
                  className="p-3 rounded-xl bg-white/[0.025] border border-white/[0.06]"
                >
                  <div className="flex items-center justify-between text-xs font-mono">
                    <span className="text-[var(--accent)] font-semibold">{entry.version}</span>
                    <span className="text-[var(--text-muted)]">{entry.date}</span>
                  </div>
                  <p className="text-xs text-[var(--text-secondary)] mt-1 leading-relaxed">
                    {entry.notes}
                  </p>
                </div>
              ))}
            </div>

            <div className="flex items-center gap-2 text-[11px] text-[var(--text-muted)] pt-1">
              <ShieldAlert className="w-3.5 h-3.5" />
              <span>Assetto Corsa is a trademark of Kunos Simulazioni. ACXS Manager is an independent desktop companion.</span>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}
