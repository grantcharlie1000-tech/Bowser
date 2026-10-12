// Banned sinks and APIs for the CSP-locked chrome page. Shared by lint.mjs (src/ and dist/app.js)
// and test/hardening.mjs, so the rule list is tested where it is defined.

export const RULES = [
  { name: 'innerHTML', re: /innerHTML/ },
  { name: 'outerHTML', re: /outerHTML/ },
  { name: 'insertAdjacentHTML', re: /insertAdjacentHTML/ },
  { name: 'eval(', re: /eval\(/ },
  { name: 'new Function', re: /new Function/ },
  { name: 'document.write', re: /document\.write/ },
  { name: 'inline event handler attribute', re: /\son[a-z]+=/ },
];

/** Every forbidden match in `text`, one entry per rule per line, with its 1-based line number. */
export function findViolations(text) {
  const violations = [];
  text.split(/\r?\n/).forEach((line, index) => {
    for (const rule of RULES) {
      if (rule.re.test(line)) violations.push({ line: index + 1, rule: rule.name });
    }
  });
  return violations;
}
