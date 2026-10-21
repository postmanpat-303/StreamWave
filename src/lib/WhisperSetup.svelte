<script lang="ts">
  /** Transcript panel setup: explains whisper-cli, shows the install command for this OS (copy / open terminal),
   *  "Set it up for me" (official download into StreamWave's folder), "I already have it" and Check again.
   *  Once whisper-cli is found it also shows where it comes from (and Remove for StreamWave's copy). The same instance
   *  stays mounted in every state so the setup dialog survives the switch to "ready". */
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { ask, open } from "@tauri-apps/plugin-dialog";
  import { api } from "./api";
  import { copyText, execCommandCopy, fmtSize, installAdvice, osKind, pathExplainer, progressPct, sourceLabel, terminalHint, WHISPER_INTRO,
    type InstallAdvice, type OsKind, type Plan, type Progress, type WhisperInfo } from "./whisperSetup";

  let { info, onchange, toast }: {
    info: WhisperInfo;
    onchange: (i: WhisperInfo) => void;
    toast: (text: string, kind?: "info" | "warn" | "err") => void;
  } = $props();

  let os = $state<OsKind>("other");
  let advice = $state<InstallAdvice | null>(null);
  let models = $state<[string, string, number][]>([]);
  let pick = $state(0); // which install option is in the code box
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;
  let hint = $state("");
  let checking = $state(false);

  onMount(() => {
    (async () => {
      try {
        const s = await api.whisperSetupInfo();
        os = osKind(s.os); models = s.models;
        advice = installAdvice(os, s.distro, !!s.binary_asset);
        if (models.length) model = models[0][0];
      } catch (e) { console.warn("whisper setup info", e); advice = installAdvice("other", null, false); }
    })();
    const un = listen<Progress>("whisper-setup-progress", (e) => { progress = e.payload; });
    return () => { un.then((f) => f()); if (copyTimer) clearTimeout(copyTimer); };
  });

  const option = $derived(advice?.options[Math.min(pick, (advice?.options.length ?? 1) - 1)] ?? null);
  /** "Set it up for me" can install the program here (an official build exists for this OS/arch). */
  const canSetup = $derived(!info.found && !!advice?.canAutoInstall);

  async function copy(text: string) {
    const ok = await copyText(text, { tauriWrite: writeText, navigatorWrite: navigator.clipboard ? (t) => navigator.clipboard.writeText(t) : undefined, execCopy: execCommandCopy });
    if (ok) { copied = true; if (copyTimer) clearTimeout(copyTimer); copyTimer = setTimeout(() => (copied = false), 1800); }
    else toast("Couldn't copy: select the command and copy it yourself", "warn");
    return ok;
  }
  async function terminal() {
    if (!option) return;
    const ok = await copy(option.command);
    try { const r = await api.openTerminal(option.command); hint = terminalHint(os, r.prefilled, ok); }
    catch (e) { hint = ""; toast(`Couldn't open a terminal: ${e}. ${ok ? "The command is copied, so paste it into one yourself." : ""}`, "warn"); }
  }
  async function checkAgain() {
    checking = true;
    try { const i = await api.whisperStatus(); onchange(i); if (!i.found) toast("Still no whisper-cli. If you just installed it, restart StreamWave so it sees the new PATH.", "warn"); else if (i.model_found) toast("Transcription is ready", "info"); }
    finally { checking = false; }
  }
  async function haveIt() {
    const p = await open({ title: "Choose whisper-cli", multiple: false, directory: false, filters: os === "windows" ? [{ name: "whisper-cli", extensions: ["exe"] }] : undefined });
    if (!p || Array.isArray(p)) return;
    try { const i = await api.whisperChoose(p); onchange(i); hint = ""; toast(i.model_found ? "Transcription is ready" : "whisper-cli found. Now it needs a speech model.", "info"); }
    catch (e) { toast(String(e), "err"); }
  }
  async function chooseModel() {
    const p = await open({ title: "Choose a Whisper model", filters: [{ name: "Whisper model", extensions: ["bin"] }] });
    if (!p || Array.isArray(p)) return;
    const s = await api.getSettings(); s.whisper_model = p; await api.saveSettings(s);
    onchange(await api.whisperStatus());
  }
  async function remove() {
    const ok = await ask(`Remove StreamWave's copy of whisper.cpp and its speech model${info.path_added ? ", and take its folder off your PATH" : ""}? You can set it up again any time.`, { title: "Remove whisper.cpp", kind: "warning", okLabel: "Remove", cancelLabel: "Keep" });
    if (!ok) return;
    try { onchange(await api.whisperRemove()); toast("Removed StreamWave's copy of whisper.cpp", "info"); } catch (e) { toast(String(e), "err"); }
  }
  async function forget() { try { onchange(await api.whisperForgetChoice()); } catch (e) { toast(String(e), "err"); } }

  // ---- Set it up for me ----
  type Phase = "closed" | "planning" | "confirm" | "running" | "done" | "error";
  let phase = $state<Phase>("closed");
  let plan = $state<Plan | null>(null);
  let model = $state("ggml-base.en.bin");
  let addPath = $state(false);
  let progress = $state<Progress | null>(null);
  let err = $state("");
  const pct = $derived(progressPct(progress));
  const needModel = $derived(!info.model_found);

  async function startSetup() { phase = "planning"; err = ""; await replan(); }
  async function replan() {
    phase = "planning";
    try { plan = await api.whisperSetupPlan(!info.found, needModel ? model : null); phase = "confirm"; }
    catch (e) { err = String(e); phase = "error"; }
  }
  async function run() {
    progress = null; phase = "running";
    try { const i = await api.whisperSetupInstall(addPath && !!plan?.can_add_path); onchange(i); phase = "done"; }
    catch (e) {
      const m = String(e);
      if (m.includes("cancelled")) {
        phase = "closed";
        const i = await api.whisperStatus(); onchange(i);
        const hadBinary = !!plan?.downloads.some((d) => d.kind === "binary");
        toast(hadBinary && i.found ? "Setup cancelled. whisper-cli had already finished and is kept; the unfinished model download was deleted." : "Setup cancelled; the unfinished download was deleted", "warn");
      }
      else { err = m; phase = "error"; onchange(await api.whisperStatus()); }
    }
  }
  function close() { if (phase !== "running") phase = "closed"; }
  function stageText(p: Progress | null): string {
    if (!p) return "Starting…";
    if (p.stage === "verify") return `Checking ${p.name}…`;
    if (p.stage === "extract") return `Unpacking ${p.name}…`;
    return `Downloading ${p.name}: ${fmtSize(p.got)} of ${fmtSize(p.total)}`;
  }
  $effect(() => {
    if (phase === "closed") return;
    const k = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); e.preventDefault(); if (phase === "running") api.whisperSetupCancel(); else close(); } };
    window.addEventListener("keydown", k, true);
    return () => window.removeEventListener("keydown", k, true);
  });
</script>

{#if !info.found}
  <div class="ws">
    <p class="lead">{WHISPER_INTRO}</p>
    <p class="small">{pathExplainer(os)}</p>
    {#if info.chosen_missing}<p class="small warn">The whisper-cli you chose earlier isn't there any more.</p>{/if}
    {#if canSetup}
      <div class="easy">
        <button class="primary" onclick={startSetup}>Set it up for me</button>
        <span class="small">Downloads the official build and a speech model into StreamWave's folder. No admin, nothing else changes.</span>
      </div>
      <div class="or small">or install it yourself{option ? ` (${option.label})` : ""}:</div>
    {:else if option}
      <div class="or small">Install it yourself ({option.label}):</div>
    {/if}
    {#if option}
      <div class="code" role="group" aria-label="Install command">
        <code>{option.command}</code>
        <button class="copy" class:done={copied} onclick={() => copy(option.command)} aria-live="polite">{copied ? "Copied ✓" : "Copy"}</button>
      </div>
      <p class="small">{option.note}</p>
    {/if}
    {#if advice?.managerNote}<p class="small muted2">{advice.managerNote}</p>{/if}
    <div class="acts">
      <button class="small" onclick={terminal} disabled={!option} title="Open a terminal with this command ready for you to run">Open terminal</button>
      <button class="small" onclick={haveIt} title="Choose the whisper-cli program yourself">I already have it…</button>
      <button class="small" onclick={checkAgain} disabled={checking}>{checking ? "Checking…" : "Check again"}</button>
    </div>
    {#if hint}<p class="small hint">{hint}</p>{/if}
    {#if advice && advice.options.length > 1}
      <div class="alts small">Other ways:
        {#each advice.options as o, i (o.label)}{#if i !== pick}<button class="link" onclick={() => { pick = i; hint = ""; }}>{o.label}</button>{/if}{/each}
      </div>
    {/if}
    {#if !advice?.canAutoInstall && os === "macos" && needModel}
      <p class="small">After installing, StreamWave can fetch the speech model: <button class="link" onclick={startSetup}>download a model</button>.</p>
    {/if}
  </div>
{:else if !info.model_found}
  <div class="ws">
    <p class="lead">whisper-cli is ready. It needs a speech model, a file the program listens with.</p>
    <div class="easy">
      <button class="primary" onclick={startSetup}>Download a model</button>
      <span class="small">From the official whisper.cpp model page on Hugging Face (Base English, {fmtSize(models[0]?.[2] ?? 147964211)}).</span>
    </div>
    <div class="acts">
      <button class="small" onclick={chooseModel}>Choose model file…</button>
      <button class="small" onclick={checkAgain} disabled={checking}>{checking ? "Checking…" : "Check again"}</button>
    </div>
  </div>
{/if}
{#if info.found}
  <div class="foot small">
    whisper-cli: {sourceLabel(info.source)}{#if info.path_added} (also on your PATH){/if}
    <span class="path" title={info.path ?? ""}>{info.path}</span>
    {#if info.source === "managed" || info.managed}<button class="link" onclick={remove} title="Delete StreamWave's whisper.cpp folder{info.path_added ? ' and undo the PATH change' : ''}">Remove</button>{/if}
    {#if info.source === "settings"}<button class="link" onclick={forget} title="Stop using the file you chose">Forget</button>{/if}
  </div>
{/if}

{#if phase !== "closed"}
  <div class="veil" role="presentation" onclick={(e) => { if (e.target === e.currentTarget && phase !== "running") close(); }}>
    <div class="card hw" role="dialog" aria-modal="true" aria-labelledby="ws-title">
      {#if phase === "planning"}
        <h2 id="ws-title">Set up transcription</h2>
        <p class="muted">Looking up the official downloads…</p>
        <div class="spin" aria-hidden="true"></div>
        <div class="actions"><button onclick={close}>Cancel</button></div>
      {:else if phase === "confirm" && plan}
        <h2 id="ws-title">Set up transcription</h2>
        <p>StreamWave will download {plan.downloads.length === 1 ? "this file" : "these files"} over HTTPS, check {plan.downloads.length === 1 ? "it" : "them"}, and install into its own folder:</p>
        <table class="dl">
          <thead><tr><th>What</th><th>From</th><th class="r">Size</th></tr></thead>
          <tbody>
            {#each plan.downloads as d (d.url)}
              <tr>
                <td><b>{d.kind === "binary" ? "whisper.cpp program" : "Speech model"}</b><div class="sub">{d.name}{d.version ? ` · build ${d.version}` : ""}</div>
                  {#if d.sha256}<div class="sub mono" title={d.sha256}>sha256 {d.sha256.slice(0, 16)}… checked after download</div>{/if}</td>
                <td><span class="host">{d.host}</span><div class="sub" title={d.url}>{d.kind === "binary" ? "ggml-org/whisper.cpp releases" : "ggerganov/whisper.cpp models"}</div></td>
                <td class="r">{fmtSize(d.size)}</td>
              </tr>
            {/each}
          </tbody>
          <tfoot><tr><td colspan="2">Total</td><td class="r"><b>{fmtSize(plan.total)}</b></td></tr></tfoot>
        </table>
        {#if needModel && models.length > 1}
          <label for="ws-model">Speech model</label>
          <select id="ws-model" bind:value={model} onchange={replan}>
            {#each models as m (m[0])}<option value={m[0]}>{m[1]} · {fmtSize(m[2])}</option>{/each}
          </select>
        {/if}
        {#if plan.binary_note}<p class="small warn">{plan.binary_note}</p>{/if}
        <p class="small muted">Installs into <span class="mono">{plan.dest}</span>. StreamWave uses it from there, so nothing else on your computer changes{plan.can_add_path ? " unless you tick the box below" : ""}. Remove undoes it.</p>
        {#if plan.can_add_path && plan.downloads.some((d) => d.kind === "binary")}
          <label class="chk"><input type="checkbox" bind:checked={addPath} /> Also add to my PATH so other apps can use it</label>
          <p class="small muted">Adds the folder to your user PATH (no admin). Apps opened afterwards see it.</p>
        {:else if plan.downloads.some((d) => d.kind === "binary")}
          <p class="small muted">No PATH change is needed. To use this whisper-cli in a terminal too, add the folder's bin directory to PATH in your shell's startup file.</p>
        {/if}
        <div class="actions">
          <button onclick={close}>Cancel</button>
          <button class="primary" onclick={run} disabled={!plan.downloads.length}>Download &amp; set up ({fmtSize(plan.total)})</button>
        </div>
      {:else if phase === "running"}
        <h2 id="ws-title">Setting up transcription…</h2>
        <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct ?? 0}><div style:width={pct === null ? "35%" : `${pct}%`} class:indet={pct === null}></div></div>
        <p class="muted">{stageText(progress)}{pct !== null ? ` · ${pct}%` : ""}</p>
        <div class="actions"><button onclick={() => api.whisperSetupCancel()}>Cancel</button></div>
      {:else if phase === "done"}
        <h2 id="ws-title">✅ Transcription is ready</h2>
        <p>whisper.cpp{plan?.downloads.some((d) => d.kind === "model") ? " and the speech model are" : " is"} installed in StreamWave's folder and checked.{info.path_added ? " Its folder is on your PATH too." : ""}</p>
        <p class="small muted">Choose a track and press <b>Transcribe track</b>. To undo, use Remove in the Transcript panel.</p>
        <div class="actions"><button class="primary" onclick={close}>Done</button></div>
      {:else if phase === "error"}
        <h2 id="ws-title">Setup didn't finish</h2>
        <p>{err}</p>
        <p class="small muted">Nothing half-finished was left behind. You can try again, or install it yourself with the command in the panel.</p>
        <div class="actions"><button onclick={close}>Close</button><button class="primary" onclick={replan}>Try again</button></div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .ws { display: flex; flex-direction: column; gap: 8px; }
  .small { font-size: 11.5px; color: var(--muted); }
  button.small { padding: 3px 8px; font-size: 11.5px; color: var(--text); }
  .ws p { margin: 0; line-height: 1.45; }
  .lead { font-size: 12.5px; color: var(--text); }
  .warn { color: var(--warn); }
  .muted2 { color: var(--muted); font-style: italic; }
  .easy { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; padding: 8px 10px; border-radius: 8px; background: rgb(var(--accent-rgb) / .07); border: 1px solid rgb(var(--accent-rgb) / .25); }
  .easy .small { flex: 1 1 180px; }
  .or { color: var(--muted); margin-top: 2px; }
  .code { display: flex; align-items: stretch; gap: 0; border-radius: 8px; background: var(--inset-bg); box-shadow: var(--inset-shadow); border: 1px solid var(--line-dark); overflow: hidden; }
  .code code { flex: 1; min-width: 0; padding: 8px 10px; font: 12.5px/1.4 ui-monospace, "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace; color: #ffd28c; white-space: pre-wrap; word-break: break-all; user-select: all; }
  .copy { border-radius: 0; border: 0; border-left: 1px solid var(--line-dark); min-width: 0; width: 64px; flex: none; padding: 0 6px; font-size: 11.5px; }
  .copy.done { color: var(--teal); }
  .acts { display: flex; gap: 6px; flex-wrap: wrap; }
  .hint { color: var(--teal); }
  .alts { color: var(--muted); display: flex; gap: 6px; flex-wrap: wrap; align-items: baseline; }
  .link { background: none; border: 0; box-shadow: none; padding: 0; color: var(--accent); font-weight: 600; font-size: inherit; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .link:hover { color: #ffc27a; }
  .foot { display: flex; gap: 6px; align-items: baseline; flex-wrap: wrap; color: var(--muted); margin-top: 6px; }
  .foot .path { font-family: ui-monospace, Consolas, monospace; font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; min-width: 0; flex: 1 1 120px; }
  .veil { position: fixed; inset: 0; background: rgba(0,0,0,.6); display: grid; place-items: center; z-index: 130; }
  .card { padding: 22px 24px; width: min(600px, 94vw); max-height: 88vh; overflow: auto; display: flex; flex-direction: column; gap: 10px; }
  .card h2 { margin: 0; font-size: 19px; }
  .card p { margin: 0; line-height: 1.5; }
  .muted { color: var(--muted); font-size: 13px; }
  .card .small { font-size: 12px; }
  .mono { font-family: ui-monospace, Consolas, monospace; font-size: 11.5px; word-break: break-all; }
  .dl { width: 100%; border-collapse: collapse; font-size: 13px; background: var(--inset-bg); box-shadow: var(--inset-shadow); border-radius: 8px; overflow: hidden; }
  .dl th, .dl td { text-align: left; padding: 7px 10px; vertical-align: top; }
  .dl thead th { font-size: 11px; text-transform: uppercase; letter-spacing: .08em; color: var(--muted); border-bottom: 1px solid var(--line); }
  .dl tbody tr + tr td { border-top: 1px solid #ffffff0d; }
  .dl tfoot td { border-top: 1px solid var(--line); }
  .dl .r { text-align: right; white-space: nowrap; }
  .dl .sub { font-size: 11.5px; color: var(--muted); margin-top: 2px; }
  .host { color: var(--teal); font-weight: 600; }
  .chk { display: flex; gap: 8px; align-items: center; font-size: 13px; color: var(--text); margin: 4px 0 0; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 6px; flex-wrap: wrap; }
  .bar { height: 10px; border-radius: 99px; background: #0c0d0f; box-shadow: var(--inset-shadow); overflow: hidden; }
  .bar div { height: 100%; background: linear-gradient(90deg, #ffb347, var(--accent)); transition: width .2s; }
  .bar .indet { animation: slide 1.2s infinite ease-in-out; }
  @keyframes slide { from { margin-left: -35%; } to { margin-left: 100%; } }
  .spin { width: 28px; height: 28px; border: 3px solid rgba(255,255,255,.15); border-top-color: var(--accent); border-radius: 50%; animation: r 0.9s linear infinite; align-self: center; margin: 8px; }
  @keyframes r { to { transform: rotate(360deg); } }
</style>
