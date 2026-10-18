// node scripts/merge-latest-json.mjs latest.json dist/latest-darwin.json > merged.json
// Adds the macOS platform entries to a release's latest.json (versions must match).
import fs from "node:fs";
const [base, frag] = process.argv.slice(2).map((f) => JSON.parse(fs.readFileSync(f, "utf8")));
if (base.version !== frag.version) { console.error(`version mismatch: ${base.version} vs ${frag.version}`); process.exit(1); }
base.platforms = { ...base.platforms, ...frag.platforms };
process.stdout.write(JSON.stringify(base, null, 2) + "\n");
