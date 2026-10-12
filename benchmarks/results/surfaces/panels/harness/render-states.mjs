// Interaction-state shots (open note, tasks tab, settings section, row hover, focus ring).
//   node render-states.mjs <distDir> <outDir>
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [dist, out] = process.argv.slice(2);
const states = [
  ['notes-full-dark-w360$', '&act=%23pn-tab-tasks', '-tasks'],
  ['notes-full-light-w360$', '&act=.pn-note-row', '-open'],
  ['notes-full-dark-w360$', '&act=.pn-note-row', '-open'],
  ['notes-full-light-w280$', '&act=%23pn-tab-tasks', '-tasks'],
  ['settings-full-dark-w900$', '&act=.pg-nav-item:nth-child(2)', '-tabs'],
  ['settings-full-light-w900$', '&act=.pg-nav-item:nth-child(3)', '-blocking'],
  ['history-full-dark-w360$', '&reveal=1&focus=.pn-row', '-hover-focus'],
  ['history-full-light-w280$', '&reveal=1', '-hover'],
  ['bookmarks-full-dark-w360$', '&reveal=1&focus=.pn-row', '-hover-focus'],
  ['bookmarks-full-light-w360$', '&act=.pn-tools%20button', '-add'],
  ['bookmarks-full-light-w280$', '&act=.pn-tools%20button', '-add'],
];
for (const [filter, query, suffix] of states) {
  spawnSync('node', [join(here, 'render-panels.mjs'), dist, out, filter], {
    stdio: 'inherit',
    env: { ...process.env, HARNESS_QUERY: query, HARNESS_SUFFIX: suffix },
  });
}
