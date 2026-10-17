// .twt project format (JSON). Stores the edit model + effect chains (incl. learned noise
// profiles) and references to source files. Peaks are recomputed on open.
import type { Project, Source, EditTrack, Marker } from "./editModel";

export const FORMAT = "streamwave-project";
/** Projects saved before the rename to StreamWave. */
export const LEGACY_FORMATS = ["truewave-project"];
export const VERSION = 1;
export interface SourceRef { path: string; name: string; duration: number; sampleRate: number; channels: number; }
export interface ProjectFile { format: string; version: number; savedAt: string; app: string; nextId: number; tracks: EditTrack[]; markers: Marker[]; sources: SourceRef[]; }

export function serialize(p: Project, sources: Map<string, Source>, nextId: number): string {
  const used = new Set(p.tracks.flatMap((t) => t.clips.map((c) => c.src)));
  const refs: SourceRef[] = [...used].map((path) => {
    const s = sources.get(path);
    return { path, name: s?.name ?? path.split(/[\\/]/).pop()!, duration: s?.duration ?? 0, sampleRate: s?.sampleRate ?? 0, channels: s?.channels ?? 0 };
  });
  const f: ProjectFile = { format: FORMAT, version: VERSION, savedAt: new Date().toISOString(), app: "StreamWave", nextId, tracks: p.tracks, markers: p.markers ?? [], sources: refs };
  return JSON.stringify(f);
}

export function deserialize(json: string): ProjectFile {
  const f = JSON.parse(json) as ProjectFile;
  if (f.format !== FORMAT && !LEGACY_FORMATS.includes(f.format)) throw new Error("Not a StreamWave project");
  if (f.version > VERSION) throw new Error(`Project was made by a newer StreamWave (format v${f.version})`);
  if (!Array.isArray(f.tracks)) throw new Error("Project has no tracks list");
  for (const t of f.tracks) {
    if (typeof t.id !== "number" || !Array.isArray(t.clips)) throw new Error("Damaged track in project");
    for (const c of t.clips) if (typeof c.src !== "string" || !(c.end >= c.start)) throw new Error("Damaged clip in project");
    t.fx ??= [];
  }
  f.sources ??= [];
  f.markers = Array.isArray(f.markers) ? f.markers.filter((m) => typeof m.time === "number") : [];
  f.nextId = Math.max(f.nextId ?? 1, ...f.tracks.map((t) => t.id + 1), ...f.markers.map((m) => m.id + 1), 1);
  return f;
}
