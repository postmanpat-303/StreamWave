import { describe, expect, it } from "vitest";
import { BUILD_FROM_SOURCE, copyText, fmtSize, installAdvice, osKind, pathExplainer, progressPct, sourceLabel, terminalHint, type Distro } from "./whisperSetup";

const distro = (id: string, version_id = "", id_like = "", name = ""): Distro => ({ id, version_id, id_like, name });

describe("install command per OS", () => {
  it("Windows: no winget package, Scoop instead; official build can be installed", () => {
    const a = installAdvice("windows", null, true);
    expect(a.options[0].command).toBe("scoop install whisper-cpp");
    expect(a.options.map((o) => o.command).join(" ")).not.toContain("winget install");
    expect(a.managerNote).toMatch(/winget has no whisper\.cpp package/);
    expect(a.canAutoInstall).toBe(true);
  });
  it("macOS: Homebrew formula whisper.cpp, no official binary", () => {
    const a = installAdvice("macos", null, false);
    expect(a.options[0].command).toBe("brew install whisper.cpp");
    expect(a.canAutoInstall).toBe(false);
    expect(installAdvice("macos", null, true).canAutoInstall).toBe(false);
  });
  it("Arch family uses pacman whisper-cpp", () => {
    expect(installAdvice("linux", distro("arch"), true).options[0].command).toBe("sudo pacman -S whisper-cpp");
    expect(installAdvice("linux", distro("manjaro", "", "arch"), true).options[0].command).toBe("sudo pacman -S whisper-cpp");
    expect(installAdvice("linux", distro("endeavouros"), true).options[0].command).toBe("sudo pacman -S whisper-cpp");
  });
  it("Debian/Ubuntu: apt whisper.cpp-tools only on releases that have it", () => {
    expect(installAdvice("linux", distro("ubuntu", "26.04"), true).options[0].command).toBe("sudo apt install whisper.cpp-tools");
    expect(installAdvice("linux", distro("ubuntu", "26.10"), true).options[0].command).toBe("sudo apt install whisper.cpp-tools");
    expect(installAdvice("linux", distro("debian", ""), true).options[0].command).toBe("sudo apt install whisper.cpp-tools"); // testing/sid
    expect(installAdvice("linux", distro("debian", "14"), true).options[0].command).toBe("sudo apt install whisper.cpp-tools");
    const old = installAdvice("linux", distro("ubuntu", "24.04", "debian", "Ubuntu 24.04 LTS"), true);
    expect(old.options[0]).toBe(BUILD_FROM_SOURCE);
    expect(old.managerNote).toMatch(/Ubuntu 24\.04 LTS doesn't package whisper-cli/);
    expect(installAdvice("linux", distro("debian", "13"), true).options[0]).toBe(BUILD_FROM_SOURCE);
  });
  it("Debian/Ubuntu derivatives get apt with a caveat", () => {
    const a = installAdvice("linux", distro("linuxmint", "23", "ubuntu debian", "Linux Mint 23"), true);
    expect(a.options[0].command).toBe("sudo apt install whisper.cpp-tools");
    expect(a.options[0].note).toMatch(/If Linux Mint 23's release has it/);
  });
  it("Fedora: library-only package, so build or official build", () => {
    const a = installAdvice("linux", distro("fedora", "44"), true);
    expect(a.options[0]).toBe(BUILD_FROM_SOURCE);
    expect(a.managerNote).toMatch(/only contains the library/);
    expect(a.canAutoInstall).toBe(true);
  });
  it("unknown Linux and other OSes fall back to building", () => {
    expect(installAdvice("linux", null, false).options[0]).toBe(BUILD_FROM_SOURCE);
    expect(installAdvice("other", null, false).options).toEqual([BUILD_FROM_SOURCE]);
    expect(osKind("freebsd")).toBe("other");
    expect(osKind("windows")).toBe("windows");
  });
  it("build command clones the official repo and builds whisper-cli", () => {
    expect(BUILD_FROM_SOURCE.command).toContain("https://github.com/ggml-org/whisper.cpp");
    expect(BUILD_FROM_SOURCE.note).toContain("whisper.cpp/build/bin/whisper-cli");
  });
});

describe("texts and formatting", () => {
  it("explains PATH per OS", () => {
    expect(pathExplainer("windows")).toMatch(/^Windows looks for programs in a list of folders called PATH/);
    expect(pathExplainer("macos")).toMatch(/^Your Mac looks/);
  });
  it("labels sources", () => {
    expect(sourceLabel("managed")).toBe("StreamWave's own copy");
    expect(sourceLabel("path")).toBe("found on PATH");
    expect(sourceLabel(null)).toBe("");
  });
  it("formats sizes", () => {
    expect(fmtSize(147964211)).toBe("141 MB");
    expect(fmtSize(10364195)).toBe("9.9 MB");
    expect(fmtSize(0)).toBe("0 MB");
    expect(fmtSize(2 * 1024 ** 3)).toBe("2.00 GB");
  });
  it("overall progress", () => {
    expect(progressPct(null)).toBeNull();
    expect(progressPct({ stage: "download", name: "x", got: 5, total: 10, done_bytes: 50, all_bytes: 200 })).toBe(25);
    expect(progressPct({ stage: "verify", name: "x", got: 5, total: 10, done_bytes: 300, all_bytes: 200 })).toBe(100);
  });
  it("terminal hints", () => {
    expect(terminalHint("linux", true, true)).toMatch(/typed in/);
    expect(terminalHint("windows", false, true)).toMatch(/right-click or Ctrl\+V/);
    expect(terminalHint("macos", false, true)).toMatch(/⌘V/);
    expect(terminalHint("macos", false, false)).toMatch(/Copy the command above/);
  });
});

describe("copy with fallbacks", () => {
  it("uses the Tauri plugin first", async () => {
    const got: string[] = [];
    expect(await copyText("cmd", { tauriWrite: async (t) => { got.push("tauri:" + t); }, navigatorWrite: async (t) => { got.push("nav:" + t); } })).toBe(true);
    expect(got).toEqual(["tauri:cmd"]);
  });
  it("falls back to navigator.clipboard, then execCommand", async () => {
    const got: string[] = [];
    const fail = async () => { throw new Error("no"); };
    expect(await copyText("a", { tauriWrite: fail, navigatorWrite: async (t) => { got.push("nav:" + t); } })).toBe(true);
    expect(await copyText("b", { tauriWrite: fail, navigatorWrite: fail, execCopy: (t) => { got.push("exec:" + t); return true; } })).toBe(true);
    expect(got).toEqual(["nav:a", "exec:b"]);
    expect(await copyText("c", { tauriWrite: fail, navigatorWrite: fail, execCopy: () => { throw new Error("x"); } })).toBe(false);
    expect(await copyText("d", {})).toBe(false);
  });
});
