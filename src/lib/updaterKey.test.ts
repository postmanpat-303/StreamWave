import { describe, expect, it } from "vitest";
import tauriConfRaw from "../../src-tauri/tauri.conf.json?raw";
import knownGood from "./fixtures/updater-pubkey.txt?raw";

// Every installed StreamWave (0.6.0 on) only accepts updates signed with the private half of this key.
// Changing the pubkey in tauri.conf.json breaks in-app updates for every existing user, so it must never be
// regenerated or "rotated". The fixture is the public key only (no private material).
describe("updater public key", () => {
  const pubkey: string = JSON.parse(tauriConfRaw).plugins.updater.pubkey;
  it("matches the known-good key that signed 0.6.0-0.8.4", () => {
    expect(pubkey.trim()).toBe(knownGood.trim());
  });
  it("is minisign key 23E8AE653A1C054C", () => {
    const text = atob(pubkey.trim());
    expect(text.split("\n")[0]).toBe("untrusted comment: minisign public key: 23E8AE653A1C054C");
    const raw = Uint8Array.from(atob(text.split("\n")[1].trim()), (c) => c.charCodeAt(0));
    const id = Array.from(raw.slice(2, 10)).reverse().map((b) => b.toString(16).padStart(2, "0")).join("").toUpperCase();
    expect(id).toBe("23E8AE653A1C054C");
  });
});
