import { describe, expect, it } from "vitest";
import capRaw from "../../src-tauri/capabilities/default.json?raw";
import { ALLOWED_LINKS, COFFEE_URL, PAYPAL_URL, REPO_URL } from "./about";

describe("about links", () => {
  it("are https and exactly the opener allowlist in the Tauri capability", () => {
    const cap = JSON.parse(capRaw);
    const opener = cap.permissions.filter((p: unknown) => typeof p === "object" && (p as { identifier: string }).identifier.startsWith("opener:"));
    expect(opener).toHaveLength(1);
    expect(opener[0].identifier).toBe("opener:allow-open-url");
    expect(opener[0].allow.map((a: { url: string }) => a.url).sort()).toEqual([...ALLOWED_LINKS].sort());
    // no broad opener permissions (open any url / path / reveal)
    expect(cap.permissions.some((p: unknown) => typeof p === "string" && p.startsWith("opener:"))).toBe(false);
    for (const u of ALLOWED_LINKS) expect(u.startsWith("https://")).toBe(true);
    expect(ALLOWED_LINKS).toHaveLength(3);
    expect(COFFEE_URL).toBe("https://buymeacoffee.com/postmanpat");
    expect(PAYPAL_URL).toBe("https://paypal.me/derekh100");
    expect(REPO_URL).toBe("https://github.com/postmanpat-303/StreamWave");
  });
});
