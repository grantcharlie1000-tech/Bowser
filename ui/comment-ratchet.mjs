import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { COMMENT_SCOPE, findComments, reconcileCommentBaseline } from './comment-rules.mjs';

const uiRoot = new URL('./', import.meta.url);
const baselineFile = new URL('comment-baseline.json', uiRoot);

function inventory(revision) {
  return Object.fromEntries(COMMENT_SCOPE.map((path) => {
    const text = revision
      ? execFileSync('git', ['show', '--end-of-options', `${revision}:ui/${path}`], {
        cwd: uiRoot,
        encoding: 'utf8',
      })
      : readFileSync(new URL(path, uiRoot), 'utf8');
    return [path, findComments(path, text)];
  }));
}

function serialize(baseline) {
  return `${JSON.stringify(baseline, null, 2)}\n`;
}

function checkBaseline() {
  const baseline = JSON.parse(readFileSync(baselineFile, 'utf8'));
  const { violations, remaining } = reconcileCommentBaseline(inventory(), baseline);
  for (const { path, line } of violations) {
    console.error(`${path}:${line}: new source comment in the Settings UI`);
  }
  return { violations, remaining, changed: serialize(baseline) !== serialize(remaining) };
}

export function checkSettingsComments() {
  const { violations, changed } = checkBaseline();
  if (changed) console.error('comment-baseline.json: run node comment-ratchet.mjs --prune from ui/');
  return violations.length + Number(changed);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [command, revision, ...extra] = process.argv.slice(2);
  if (command === '--inventory' && !extra.length) {
    process.stdout.write(serialize(inventory(revision)));
  } else if (command === '--prune' && !revision) {
    const { violations, remaining, changed } = checkBaseline();
    if (changed) writeFileSync(baselineFile, serialize(remaining));
    if (violations.length) process.exitCode = 1;
  } else {
    console.error('Usage: node comment-ratchet.mjs --inventory [git-ref] | --prune');
    process.exitCode = 1;
  }
}
