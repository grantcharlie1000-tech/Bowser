import { createHash } from 'node:crypto';
import { tokenize, TokenType } from '@csstools/css-tokenizer';
import ts from 'typescript';

export const COMMENT_SCOPE = [
  'src/address-settings-logic.ts',
  'src/feeds-settings-logic.ts',
  'src/settings-page-logic.ts',
  'src/settings-page.ts',
  'src/settings-profiles.ts',
  'src/settings-start.ts',
  'src/settings-themes.ts',
  'src/settings-tiles.ts',
  'src/styles/settings.css',
  'src/tab-options-logic.ts',
  'src/theme-options-logic.ts',
];

function typescriptCommentRanges(path, text) {
  const sourceFile = ts.createSourceFile(path, text, ts.ScriptTarget.Latest);
  const literalEnds = new Map();
  function visit(node) {
    if (ts.isRegularExpressionLiteral(node) || ts.isTemplateLiteralToken(node)) {
      const start = node.getStart(sourceFile);
      if (start < node.end) literalEnds.set(start, node.end);
    }
    ts.forEachChild(node, visit);
  }
  visit(sourceFile);

  const scanner = ts.createScanner(ts.ScriptTarget.Latest, false, ts.LanguageVariant.Standard, text);
  const ranges = [];
  while (true) {
    const literalEnd = literalEnds.get(scanner.getTokenEnd());
    if (literalEnd !== undefined) {
      scanner.resetTokenState(literalEnd);
      continue;
    }
    const token = scanner.scan();
    if (token === ts.SyntaxKind.EndOfFileToken) break;
    if (token === ts.SyntaxKind.SingleLineCommentTrivia || token === ts.SyntaxKind.MultiLineCommentTrivia) {
      ranges.push([scanner.getTokenStart(), scanner.getTokenEnd()]);
    }
  }
  return ranges;
}

export function findComments(path, text) {
  const ranges = path.endsWith('.css')
    ? tokenize({ css: text })
      .filter(([kind]) => kind === TokenType.Comment)
      .map(([, , start, end]) => [start, end + 1])
    : typescriptCommentRanges(path, text);
  const lineBreak = /\r\n?|\n|\u2028|\u2029/g;
  let nextBreak = lineBreak.exec(text);
  let line = 1;
  function lineAt(offset) {
    while (nextBreak && nextBreak.index + nextBreak[0].length <= offset) {
      line += 1;
      nextBreak = lineBreak.exec(text);
    }
    return line;
  }
  return ranges.map(([start, end]) => ({
    start,
    end,
    line: lineAt(start),
    endLine: lineAt(end - 1),
    text: text.slice(start, end),
  }));
}

function fingerprint(text) {
  return createHash('sha256').update(text.replace(/\r\n?/g, '\n')).digest('hex');
}

export function reconcileCommentBaseline(inventory, baseline) {
  const violations = [];
  const remaining = {};
  for (const [path, comments] of Object.entries(inventory)) {
    const allowed = baseline[path] ?? {};
    const counts = {};
    for (const comment of comments) {
      const hash = fingerprint(comment.text);
      const count = counts[hash] ?? 0;
      if (count < (allowed[hash] ?? 0)) {
        counts[hash] = count + 1;
      } else {
        violations.push({ path, line: comment.line });
      }
    }
    const entries = Object.entries(counts).sort(([first], [second]) => first.localeCompare(second));
    if (entries.length) remaining[path] = Object.fromEntries(entries);
  }
  return { violations, remaining };
}
