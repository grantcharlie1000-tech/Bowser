// Entry point for `npm run e2e`: zero-flash harness, then interaction tests. Exit code is non-zero if either fails.
// Flags are forwarded (e.g. --detection=stub). Headless only; never opens a window or sends OS input.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";
const HERE = dirname(fileURLToPath(import.meta.url));
try { os.setPriority(process.pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch (e) { /* ignore */ }
let code = 0;
for (const script of ["zero-flash.mjs", "interaction.mjs", "page-runtime.mjs", "theme.mjs", "bowser-feed.mjs", "cards.mjs", "heat.mjs", "youtube.mjs"]) {
  const r = spawnSync(process.execPath, [join(HERE, script), ...process.argv.slice(2)], { stdio: "inherit" });
  if (r.status !== 0) code = r.status || 1;
}
process.exit(code);
