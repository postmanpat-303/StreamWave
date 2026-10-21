/** Transcript setup helpers: which install command to show per OS, friendly explanations, sizes, clipboard. */

export type OsKind = "windows" | "macos" | "linux" | "other";
export interface Distro { id: string; id_like: string; version_id: string; name: string }
export interface InstallOption { label: string; command: string; note: string }
export interface InstallAdvice {
  /** best first; shown in the code box */
  options: InstallOption[];
  /** extra context about packages (e.g. why there's no winget command) */
  managerNote: string | null;
  /** an official ready-made whisper-cli exists for this OS/arch, so "Set it up for me" can install the program */
  canAutoInstall: boolean;
}
export type Source = "settings" | "managed" | "bundled" | "path";
export interface WhisperInfo {
  found: boolean; path: string | null; source: Source | null;
  model_found: boolean; model_path: string | null; message: string;
  managed: boolean; path_added: boolean; chosen_missing: boolean;
}
export interface Download { kind: "binary" | "model"; name: string; url: string; host: string; size: number; sha256: string | null; version: string | null }
export interface Plan { os: string; arch: string; downloads: Download[]; total: number; dest: string; binary_note: string | null; can_add_path: boolean }
export interface Progress { stage: "download" | "verify" | "extract"; name: string; got: number; total: number; done_bytes: number; all_bytes: number }

export function osKind(os: string): OsKind {
  return os === "windows" || os === "macos" || os === "linux" ? os : "other";
}

/** Build-from-source works everywhere (needs git, cmake and a C++ compiler). */
export const BUILD_FROM_SOURCE: InstallOption = {
  label: "Build from source",
  command: "git clone --depth 1 https://github.com/ggml-org/whisper.cpp && cmake -S whisper.cpp -B whisper.cpp/build && cmake --build whisper.cpp/build -j --config Release",
  note: "Needs git, cmake and a C++ compiler. The program ends up in whisper.cpp/build/bin/whisper-cli: choose it with “I already have it”.",
};

const verAtLeast = (v: string, min: number[]) => {
  const p = v.split(".").map((x) => parseInt(x, 10));
  if (p.some((x) => Number.isNaN(x))) return false;
  for (let i = 0; i < min.length; i++) { const a = p[i] ?? 0; if (a !== min[i]) return a > min[i]; }
  return true;
};

/**
 * Install commands, checked in Oct 2026:
 * - winget: no whisper.cpp package (only third-party apps that bundle it), so Windows uses Scoop's main bucket
 *   (`whisper-cpp`, which installs the official whisper-bin-x64.zip) or "Set it up for me".
 * - Homebrew: formula `whisper.cpp` (the old name `whisper-cpp` redirects to it); installs whisper-cli.
 * - Arch: `whisper-cpp` in extra. Debian 14 / unstable and Ubuntu 26.04+: `whisper.cpp-tools` (/usr/bin/whisper-cli).
 * - Fedora's `whisper-cpp` is only the library (no whisper-cli), so build from source or use the official build.
 */
export function installAdvice(os: OsKind, distro: Distro | null, hasOfficialBinary: boolean): InstallAdvice {
  if (os === "windows") return {
    options: [{ label: "Scoop", command: "scoop install whisper-cpp",
      note: "Scoop's main bucket installs the official whisper.cpp build and puts whisper-cli on PATH. No Scoop yet? Get it from scoop.sh, or use “Set it up for me”." }],
    managerNote: "winget has no whisper.cpp package (only other apps that bundle it), so “winget install whisper.cpp” won't work.",
    canAutoInstall: hasOfficialBinary,
  };
  if (os === "macos") return {
    options: [{ label: "Homebrew", command: "brew install whisper.cpp",
      note: "Needs Homebrew (brew.sh). It puts whisper-cli in Homebrew's folder, which StreamWave checks automatically." }, BUILD_FROM_SOURCE],
    managerNote: "whisper.cpp doesn't publish a ready-made Mac program, so Homebrew is the easy way. “Set it up for me” can still download the speech model.",
    canAutoInstall: false,
  };
  if (os === "linux") {
    const d = distro ?? { id: "", id_like: "", version_id: "", name: "" };
    const family = `${d.id} ${d.id_like}`.toLowerCase().split(/\s+/);
    const name = d.name || "your distro";
    const apt: InstallOption = { label: "APT", command: "sudo apt install whisper.cpp-tools",
      note: "Debian 14 (forky) / unstable and Ubuntu 26.04 or newer have it; it installs /usr/bin/whisper-cli." };
    if (family.includes("arch") || ["manjaro", "endeavouros", "garuda", "cachyos"].includes(d.id))
      return { options: [{ label: "pacman", command: "sudo pacman -S whisper-cpp", note: "From Arch's extra repository; it installs /usr/bin/whisper-cli." }, BUILD_FROM_SOURCE], managerNote: null, canAutoInstall: hasOfficialBinary };
    if (d.id === "ubuntu" || d.id === "debian") {
      const has = d.id === "ubuntu" ? verAtLeast(d.version_id, [26, 4]) : (d.version_id === "" || verAtLeast(d.version_id, [14]));
      if (has) return { options: [apt, BUILD_FROM_SOURCE], managerNote: null, canAutoInstall: hasOfficialBinary };
      return { options: [BUILD_FROM_SOURCE, apt], canAutoInstall: hasOfficialBinary,
        managerNote: `${name} doesn't package whisper-cli yet (it arrives in Debian 14 and Ubuntu 26.04), so “Set it up for me” is the easy way.` };
    }
    if (family.includes("debian") || family.includes("ubuntu"))
      return { options: [{ ...apt, note: `If ${name}'s release has it (it's based on Debian 14 / Ubuntu 26.04 or newer). ${apt.note}` }, BUILD_FROM_SOURCE], managerNote: null, canAutoInstall: hasOfficialBinary };
    if (family.includes("fedora") || family.includes("rhel"))
      return { options: [BUILD_FROM_SOURCE], canAutoInstall: hasOfficialBinary,
        managerNote: "Fedora's whisper-cpp package only contains the library, not the whisper-cli program, so use “Set it up for me” or build it." };
    return { options: [BUILD_FROM_SOURCE], managerNote: null, canAutoInstall: hasOfficialBinary };
  }
  return { options: [BUILD_FROM_SOURCE], managerNote: null, canAutoInstall: false };
}

/** One or two friendly sentences about what "on PATH" means. */
export function pathExplainer(os: OsKind): string {
  const where = os === "windows" ? "Windows looks" : os === "macos" ? "Your Mac looks" : "Your computer looks";
  return `${where} for programs in a list of folders called PATH. whisper-cli needs to be in one of them, or StreamWave needs to know exactly where it is (StreamWave's own setup does that for you).`;
}

export const WHISPER_INTRO = "Transcripts use whisper-cli, the free speech-to-text program from the whisper.cpp project. It runs on your computer: nothing is uploaded.";

export function sourceLabel(s: Source | null): string {
  switch (s) {
    case "settings": return "the file you chose";
    case "managed": return "StreamWave's own copy";
    case "bundled": return "next to StreamWave";
    case "path": return "found on PATH";
    default: return "";
  }
}

export function fmtSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 MB";
  const mb = bytes / 1048576;
  if (mb >= 1000) return `${(mb / 1024).toFixed(2)} GB`;
  return `${mb >= 100 ? Math.round(mb) : mb.toFixed(1)} MB`;
}

/** Overall percentage of a running setup. */
export function progressPct(p: Progress | null): number | null {
  if (!p || p.all_bytes <= 0) return null;
  return Math.max(0, Math.min(100, Math.round((p.done_bytes / p.all_bytes) * 100)));
}

export interface ClipboardDeps {
  tauriWrite?: (t: string) => Promise<void>;
  navigatorWrite?: (t: string) => Promise<void>;
  execCopy?: (t: string) => boolean;
}
/** Copy with fallbacks: Tauri clipboard plugin, then navigator.clipboard, then a hidden textarea + execCommand. */
export async function copyText(text: string, deps: ClipboardDeps): Promise<boolean> {
  for (const f of [deps.tauriWrite, deps.navigatorWrite]) {
    if (!f) continue;
    try { await f(text); return true; } catch { /* next */ }
  }
  try { return deps.execCopy ? deps.execCopy(text) : false; } catch { return false; }
}

export function execCommandCopy(text: string): boolean {
  const ta = document.createElement("textarea");
  ta.value = text; ta.setAttribute("readonly", ""); ta.style.position = "fixed"; ta.style.opacity = "0";
  document.body.appendChild(ta); ta.select();
  try { return document.execCommand("copy"); } finally { ta.remove(); }
}

/** What to tell the user after "Open terminal". */
export function terminalHint(os: OsKind, prefilled: boolean, copied: boolean): string {
  if (prefilled) return "A terminal opened with the command typed in. Check it, then press Enter to run it.";
  const paste = os === "macos" ? "⌘V" : os === "windows" ? "right-click or Ctrl+V" : "Ctrl+Shift+V";
  return copied ? `A terminal opened and the command is copied: paste it (${paste}), check it, then press Enter.`
    : "A terminal opened. Copy the command above, paste it there and press Enter.";
}
