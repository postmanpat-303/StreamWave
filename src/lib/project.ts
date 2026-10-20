// .twt project format (JSON). Stores the edit model + effect chains (incl. learned noise
// profiles) and references to source files. Peaks are recomputed on open.
import { usesClipFeatures, usesCrossfades, type Project, type Source, type EditTrack, type Marker } from "./editModel";

export const FORMAT = "streamwave-project";
/** Projects saved before the rename to StreamWave. */
export const LEGACY_FORMATS = ["truewave-project"];
/** Newest format this build reads. v2 (0.10.0) = gap clips and per-clip effect stacks. */
export const VERSION = 3;
export interface SourceRef { path: string; name: string; duration: number; sampleRate: number; channels: number; }
export interface ProjectFile { format: string; version: number; savedAt: string; app: string; nextId: number; tracks: EditTrack[]; markers: Marker[]; sources: SourceRef[]; }

export function serialize(p: Project, sources: Map<string, Source>, nextId: number): string {
  const used = new Set(p.tracks.flatMap((t) => t.clips.filter((c) => !c.gap).map((c) => c.src)));
  const refs: SourceRef[] = [...used].map((path) => {
    const s = sources.get(path);
    return { path, name: s?.name ?? path.split(/[\\/]/).pop()!, duration: s?.duration ?? 0, sampleRate: s?.sampleRate ?? 0, channels: s?.channels ?? 0 };
  });
    // only projects that need the new clip data are marked v2, so simple projects still open in older versions
  const f: ProjectFile = { format: FORMAT, version: usesCrossfades(p) ? 3 : usesClipFeatures(p) ? 2 : 1, savedAt: new Date().toISOString(), app: "StreamWave", nextId, tracks: p.tracks, markers: p.markers ?? [], sources: refs };
  return JSON.stringify(f);
}

export function deserialize(json: string): ProjectFile {
  const f = JSON.parse(json) as ProjectFile;
  if (f.format !== FORMAT && !LEGACY_FORMATS.includes(f.format)) throw new Error("Not a StreamWave project");
  if (f.version > VERSION) throw new Error(`Project was made by a newer StreamWave (format v${f.version})`);
  if (!Array.isArray(f.tracks)) throw new Error("Project has no tracks list");
  for (const t of f.tracks) {
    if (typeof t.id !== "number" || !Array.isArray(t.clips)) throw new Error("Damaged track in project");
    for (const c of t.clips) {
      if (typeof c.src !== "string" || !(c.end >= c.start) || (!c.gap && !c.src)) throw new Error("Damaged clip in project");
      if (c.fx !== undefined && !Array.isArray(c.fx)) throw new Error("Damaged clip effects in project");
    }
    t.fx ??= [];
  }
  f.sources ??= [];
  f.markers = Array.isArray(f.markers) ? f.markers.filter((m) => typeof m.time === "number") : [];
  f.nextId = Math.max(f.nextId ?? 1, ...f.tracks.map((t) => t.id + 1), ...f.markers.map((m) => m.id + 1), 1);
  return f;
}
