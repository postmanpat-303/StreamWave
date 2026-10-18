/** Update-check helpers (pure, unit-tested). The download itself is done by @tauri-apps/plugin-updater. */

export const DAY_MS = 24 * 60 * 60 * 1000;

interface Ver { nums: number[]; pre: string[] }

function parse(v: string): Ver | null {
  const m = /^\s*v?(\d+(?:\.\d+)*)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?\s*$/.exec(v ?? "");
  if (!m) return null;
  return { nums: m[1].split(".").map(Number), pre: m[2] ? m[2].split(".") : [] };
}

/** Semver-style comparison: -1 if a < b, 0 if equal, 1 if a > b. A leading "v" is ignored; 1.0.0-beta < 1.0.0. Unparseable versions sort lowest. */
export function compareVersions(a: string, b: string): -1 | 0 | 1 {
  const x = parse(a), y = parse(b);
  if (!x || !y) return x ? 1 : y ? -1 : 0;
  for (let i = 0; i < Math.max(x.nums.length, y.nums.length, 3); i++) {
    const d = (x.nums[i] ?? 0) - (y.nums[i] ?? 0);
    if (d) return d > 0 ? 1 : -1;
  }
  if (!x.pre.length || !y.pre.length) return x.pre.length === y.pre.length ? 0 : x.pre.length ? -1 : 1;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i], q = y.pre[i];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const pn = /^\d+$/.test(p), qn = /^\d+$/.test(q);
    if (pn && qn) { const d = Number(p) - Number(q); if (d) return d > 0 ? 1 : -1; }
    else if (pn !== qn) return pn ? -1 : 1;
    else if (p !== q) return p < q ? -1 : 1;
  }
  return 0;
}

export const isNewer = (latest: string, current: string) => compareVersions(latest, current) > 0;

/** Quiet startup check: only when enabled and the last automatic check was over a day ago (or never, or in the future = clock change). */
export function shouldAutoCheck(enabled: boolean, lastCheck: number | null | undefined, now: number): boolean {
  if (!enabled) return false;
  if (!lastCheck) return true;
  return now - lastCheck >= DAY_MS || lastCheck > now;
}

/** Should we tell the user about `latest`? Manual checks always show a newer version; quiet checks respect "Skip this version". */
export function shouldOffer(latest: string, current: string, skipped: string | null | undefined, manual: boolean): boolean {
  if (!isNewer(latest, current)) return false;
  if (manual) return true;
  return !skipped || compareVersions(latest, skipped) !== 0;
}

/** Friendly wording for a failed check (offline, GitHub down, no latest.json yet, unsupported package…). */
export function friendlyError(e: unknown): string {
  const s = String(e ?? "").toLowerCase();
  if (/network|dns|connect|offline|timed? ?out|resolve|unreachable|error sending request|load failed|failed to fetch|abort/.test(s))
    return "Couldn't reach GitHub to check for updates. Check your internet connection and try again later.";
  if (/404|not found|release json|could not fetch a valid/.test(s))
    return "Couldn't find update information right now. Please try again later.";
  if (/signature/.test(s)) return "The downloaded update couldn't be verified, so it wasn't installed. Please try again later or download it from GitHub.";
  return "Couldn't check for updates right now. Please try again later.";
}

/** GitHub's "latest release" API (CORS-enabled, no token needed). Used on macOS, where there is no in-app updater yet. */
export const LATEST_RELEASE_API = "https://api.github.com/repos/postmanpat-303/StreamWave/releases/latest";

/** True in the macOS app (WKWebView user agent). Updates are manual there for now: latest.json has no macOS entry. */
export const isMacUserAgent = (ua: string) => /Macintosh|Mac OS X/.test(ua ?? "") && !/iPhone|iPad|Windows|Linux/.test(ua ?? "");

export interface ReleaseInfo { version: string; body: string }

/** Version and notes from GitHub's latest-release JSON; null for drafts, pre-releases or anything unexpected. */
export function parseLatestRelease(j: unknown): ReleaseInfo | null {
  const o = (j ?? {}) as { tag_name?: unknown; body?: unknown; draft?: unknown; prerelease?: unknown };
  if (typeof o.tag_name !== "string" || o.draft || o.prerelease) return null;
  const version = o.tag_name.trim().replace(/^v/i, "");
  if (!/^\d+(\.\d+)+/.test(version)) return null;
  return { version, body: typeof o.body === "string" ? o.body : "" };
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

function inline(s: string): string {
  // s is already HTML-escaped; only add our own tags
  return s
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
    .replace(/(^|[^*])\*([^*\s][^*]*)\*/g, "$1<i>$2</i>")
    // links are shown as text only (the app may not navigate or open arbitrary URLs)
    .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<span class="mdlink" title="$2">$1</span>');
}

/** Minimal, safe Markdown → HTML for release notes. All input is escaped first; output only contains
 *  h3/h4, p, ul/ol/li, b, i, code, span and table tags that we generate. No raw HTML, no links, no images. */
export function renderMarkdown(md: string): string {
  const out: string[] = [];
  let list: "ul" | "ol" | null = null;
  let table = false;
  const close = () => { if (list) { out.push(`</${list}>`); list = null; } if (table) { out.push("</table>"); table = false; } };
  for (const raw of (md ?? "").replace(/\r\n?/g, "\n").split("\n")) {
    const line = esc(raw.trimEnd());
    let m: RegExpExecArray | null;
    if (!line.trim()) { close(); continue; }
    if ((m = /^\s*(#{1,6})\s+(.*)$/.exec(line))) { close(); out.push(`<h${m[1].length <= 2 ? 3 : 4}>${inline(m[2])}</h${m[1].length <= 2 ? 3 : 4}>`); continue; }
    if (/^\s*\|.*\|\s*$/.test(line)) {
      if (/^\s*\|[\s:|-]+\|\s*$/.test(line)) continue; // separator row
      if (list) { out.push(`</${list}>`); list = null; }
      if (!table) { out.push("<table>"); table = true; }
      out.push("<tr>" + line.trim().slice(1, -1).split("|").map((c) => `<td>${inline(c.trim())}</td>`).join("") + "</tr>");
      continue;
    }
    if ((m = /^\s*[-*+]\s+(.*)$/.exec(line)) || (m = /^\s*\d+[.)]\s+(.*)$/.exec(line))) {
      const kind = /^\s*\d/.test(line) ? "ol" : "ul";
      if (table) { out.push("</table>"); table = false; }
      if (list !== kind) { if (list) out.push(`</${list}>`); out.push(`<${kind}>`); list = kind; }
      out.push(`<li>${inline(m[1])}</li>`);
      continue;
    }
    if ((m = /^\s*&gt;\s?(.*)$/.exec(line))) { close(); out.push(`<p class="quote">${inline(m[1])}</p>`); continue; }
    close();
    out.push(`<p>${inline(line.trim())}</p>`);
  }
  close();
  return out.join("\n");
}
