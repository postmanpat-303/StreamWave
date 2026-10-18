import { describe, expect, it } from "vitest";
import { compareVersions, isNewer, shouldAutoCheck, shouldOffer, renderMarkdown, friendlyError, DAY_MS, isMacUserAgent, parseLatestRelease, LATEST_RELEASE_API, manualUpdateReason } from "./updates";

describe("compareVersions", () => {
  it("orders release versions numerically", () => {
    expect(compareVersions("0.6.0", "0.5.4")).toBe(1);
    expect(compareVersions("0.5.10", "0.5.9")).toBe(1);
    expect(compareVersions("1.0.0", "0.99.99")).toBe(1);
    expect(compareVersions("0.6.0", "0.6.0")).toBe(0);
    expect(compareVersions("0.5.4", "0.6.0")).toBe(-1);
  });
  it("ignores a leading v and build metadata, pads missing parts", () => {
    expect(compareVersions("v0.6.0", "0.6.0")).toBe(0);
    expect(compareVersions("0.6", "0.6.0")).toBe(0);
    expect(compareVersions("0.6.0+build.7", "0.6.0")).toBe(0);
  });
  it("puts pre-releases before the release", () => {
    expect(compareVersions("0.7.0-beta.1", "0.7.0")).toBe(-1);
    expect(compareVersions("0.7.0-beta.2", "0.7.0-beta.1")).toBe(1);
    expect(compareVersions("0.7.0-beta.10", "0.7.0-beta.9")).toBe(1);
    expect(compareVersions("0.7.0-alpha", "0.7.0-beta")).toBe(-1);
    expect(compareVersions("0.7.0-beta.1", "0.6.9")).toBe(1);
  });
  it("treats garbage as lowest", () => {
    expect(compareVersions("nonsense", "0.1.0")).toBe(-1);
    expect(isNewer("", "0.6.0")).toBe(false);
  });
});

describe("shouldAutoCheck", () => {
  const now = Date.UTC(2026, 9, 17, 21, 0);
  it("respects the setting", () => expect(shouldAutoCheck(false, null, now)).toBe(false));
  it("checks when never checked", () => expect(shouldAutoCheck(true, null, now)).toBe(true));
  it("at most once a day", () => {
    expect(shouldAutoCheck(true, now - 3600_000, now)).toBe(false);
    expect(shouldAutoCheck(true, now - DAY_MS + 1, now)).toBe(false);
    expect(shouldAutoCheck(true, now - DAY_MS, now)).toBe(true);
  });
  it("recovers from a clock that went backwards", () => expect(shouldAutoCheck(true, now + 5 * DAY_MS, now)).toBe(true));
});

describe("shouldOffer / skip this version", () => {
  it("never offers the same or an older version", () => {
    expect(shouldOffer("0.6.0", "0.6.0", null, true)).toBe(false);
    expect(shouldOffer("0.5.4", "0.6.0", null, true)).toBe(false);
  });
  it("offers a newer version", () => expect(shouldOffer("0.6.1", "0.6.0", null, false)).toBe(true));
  it("quiet check hides a skipped version", () => {
    expect(shouldOffer("0.6.1", "0.6.0", "0.6.1", false)).toBe(false);
    expect(shouldOffer("0.6.1", "0.6.0", "v0.6.1", false)).toBe(false);
  });
  it("a newer release than the skipped one is offered again", () => expect(shouldOffer("0.6.2", "0.6.0", "0.6.1", false)).toBe(true));
  it("manual check still shows a skipped version", () => expect(shouldOffer("0.6.1", "0.6.0", "0.6.1", true)).toBe(true));
});

describe("renderMarkdown", () => {
  it("escapes raw HTML and script", () => {
    const h = renderMarkdown('<script>alert(1)</script>\n<img src=x onerror="alert(1)">');
    expect(h).not.toMatch(/<script|<img/i);
    expect(h).toContain("&lt;script&gt;");
  });
  it("renders headings, lists, bold, code", () => {
    const h = renderMarkdown("## New\n- **Updates** in `Help`\n- two\n\n1. first\n\nPlain *text*");
    expect(h).toContain("<h3>New</h3>");
    expect(h).toContain("<ul>\n<li><b>Updates</b> in <code>Help</code></li>\n<li>two</li>\n</ul>");
    expect(h).toContain("<ol>\n<li>first</li>\n</ol>");
    expect(h).toContain("<p>Plain <i>text</i></p>");
  });
  it("shows links as text only, including javascript: urls", () => {
    const h = renderMarkdown("[click](javascript:alert(1)) and [site](https://example.com)");
    expect(h).not.toMatch(/<a\b|href=/i);
    expect(h).toContain('<span class="mdlink"');
    expect(h).toContain(">click</span>");
  });
  it("cannot break out of the title attribute", () => {
    const h = renderMarkdown('[x](https://e.com/"onmouseover="alert(1))');
    expect(h).not.toMatch(/"onmouseover=/);
  });
  it("renders tables", () => expect(renderMarkdown("| a | b |\n|---|---|\n| 1 | 2 |")).toBe("<table>\n<tr><td>a</td><td>b</td></tr>\n<tr><td>1</td><td>2</td></tr>\n</table>"));
});

describe("friendlyError", () => {
  it("is non-scary for offline", () => expect(friendlyError(new Error("error sending request for url"))).toMatch(/internet connection/));
  it("has a generic fallback", () => expect(friendlyError("weird")).toMatch(/try again later/));
});

describe("macOS manual updates", () => {
  const mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
  it("detects the macOS webview only", () => {
    expect(isMacUserAgent(mac)).toBe(true);
    expect(isMacUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/130.0")).toBe(false);
    expect(isMacUserAgent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)")).toBe(false);
  });
  it("reads version and notes from GitHub's latest release", () => {
    expect(parseLatestRelease({ tag_name: "v0.8.4", body: "## Notes", draft: false, prerelease: false })).toEqual({ version: "0.8.4", body: "## Notes" });
    expect(parseLatestRelease({ tag_name: "v0.9.0" })).toEqual({ version: "0.9.0", body: "" });
  });
  it("ignores drafts, pre-releases and junk", () => {
    expect(parseLatestRelease({ tag_name: "v0.9.0", draft: true })).toBeNull();
    expect(parseLatestRelease({ tag_name: "v0.9.0", prerelease: true })).toBeNull();
    expect(parseLatestRelease({ message: "Not Found" })).toBeNull();
    expect(parseLatestRelease(null)).toBeNull();
    expect(parseLatestRelease({ tag_name: "nightly" })).toBeNull();
  });
  it("uses this repo's API", () => expect(LATEST_RELEASE_API).toBe("https://api.github.com/repos/postmanpat-303/StreamWave/releases/latest"));
  it("treats WebKit fetch failures as offline", () => expect(friendlyError(new TypeError("Load failed"))).toMatch(/internet connection/));
});

describe("which installs update by hand", () => {
  const mac = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
  const linux = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)";
  const win = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/130.0";
  it("Mac always", () => { expect(manualUpdateReason(mac, "app")).toBe("mac"); expect(manualUpdateReason(mac, null)).toBe("mac"); });
  it(".deb / .rpm (and the Arch package, which carries the .deb binary) on Linux", () => {
    expect(manualUpdateReason(linux, "deb")).toBe("package");
    expect(manualUpdateReason(linux, "rpm")).toBe("package");
  });
  it("AppImage, Windows installer and dev builds keep the in-app updater", () => {
    expect(manualUpdateReason(linux, "appimage")).toBeNull();
    expect(manualUpdateReason(linux, "unknown")).toBeNull();
    expect(manualUpdateReason(linux, null)).toBeNull();
    expect(manualUpdateReason(win, "nsis")).toBeNull();
  });
});
