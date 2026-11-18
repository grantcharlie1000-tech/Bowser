#!/usr/bin/env python3
"""Builds the Bowser site: wraps each src/<page>.html in tools/layout.html and writes <page>.html
at the root. releases.html is generated from tools/releases.json (refresh it with
`python3 tools/build.py --refresh-releases`, which needs the gh CLI), and the download page
reads tools/stable.json, a copy of the repository's stable/release.json. On GitHub both are
refreshed after every release by .github/workflows/site-release-notes.yml on main.
Run from anywhere: python3 tools/build.py"""
import html, json, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = 'grantcharlie1000-tech/Bowser'

PAGES = {
    # file: (title, description, active nav key)
    'index': ('Bowser — the small, fast browser for Windows',
              'Bowser is a small, open-source browser for 64-bit Windows. It blocks ads and trackers from the first launch, keeps private tabs private, and runs your Chrome extensions.', None),
    'features': ('Features — Bowser', 'Everything Bowser does: tabs and layouts, split view, command palette, reader view, notes, profiles, passwords, extensions and more.', 'features'),
    'privacy': ('Privacy — Bowser', 'What Bowser blocks, what it keeps on your PC, and exactly what it sends over the network.', 'privacy'),
    'download': ('Download Bowser for Windows', 'Download Bowser for 64-bit Windows 10 and 11. About 10 MB, free and open source.', 'download'),
    'releases': ('Release notes — Bowser', 'What changed in each Bowser release.', 'releases'),
    'help': ('Help — Bowser', 'Keyboard shortcuts, answers to common questions, and fixes for common problems with Bowser.', 'help'),
}
NAV = [('features', 'Features'), ('privacy', 'Privacy'), ('releases', 'Release notes'), ('help', 'Help')]

# Pull requests that only move the release machinery, not the browser.
PROCESS = re.compile(r'^(pin |promote |prepare |bump |release:|recover v|add the v0\.|fix release publisher|cut the stable pin)', re.I)


def refresh_releases():
    pages = json.loads(subprocess.run(['gh', 'api', '--paginate', '--slurp', f'repos/{REPO}/releases?per_page=100'],
                                      capture_output=True, text=True, check=True).stdout)
    # Newest first. Drafts are visible to a token that can write releases; they are not released yet.
    rel = [r for page in pages for r in page if not r.get('draft')]
    out = []
    for i, r in enumerate(rel):
        prs = []
        if i + 1 < len(rel):
            cmp = json.loads(subprocess.run(['gh', 'api', f'repos/{REPO}/compare/{rel[i+1]["tag_name"]}...{r["tag_name"]}',
                                             '--jq', '{c:[.commits[].commit.message]}'], capture_output=True, text=True, check=True).stdout)
            for m in cmp['c']:
                mm = re.match(r'Merge pull request #(\d+) from \S+\n\n(.+)', m)
                if mm:
                    prs.append([int(mm.group(1)), mm.group(2).strip().split('\n')[0]])
        out.append(dict(tag=r['tag_name'], date=r['published_at'][:10], prs=prs, body=r['body'] or '',
                        exe=any(a['name'] == 'Bowser.exe' for a in r['assets'])))
    with open(os.path.join(ROOT, 'tools', 'releases.json'), 'w') as f:
        json.dump(out, f, indent=1)


def summary_of(body):
    """The first paragraph of a hand-written release body, or '' for the automated boilerplate."""
    if not body or body.startswith('Verified Windows HTML-chrome build'):
        return ''
    para = body.replace('\r', '').strip().split('\n\n')[0].strip()
    # GitHub's generated notes ("## What's Changed", a "* ... by @... in https://..." list) and any
    # other Markdown heading or list are not a summary: the pull requests are listed under it anyway.
    if re.match(r'(#|[*-] )', para):
        return ''
    para = re.sub(r'\s*Built (with|from) .*$', '', para)
    return para


def inline(text):
    text = html.escape(text, quote=False)
    text = re.sub(r'`([^`]+)`', r'<code>\1</code>', text)
    return re.sub(r'\*\*([^*]+)\*\*', r'<b>\1</b>', text)


def releases_body():
    data = json.load(open(os.path.join(ROOT, 'tools', 'releases.json')))
    pin = {}
    try:
        pin = json.load(open(os.path.join(ROOT, 'tools', 'stable.json')))
    except OSError:
        pass
    items = []
    for r in data:
        changes = [(n, t) for n, t in r['prs'] if not PROCESS.match(t)]
        summary = summary_of(r['body'])
        if not changes and not summary:
            continue
        month = __import__('datetime').date.fromisoformat(r['date']).strftime('%-d %B %Y')
        badge = '<span class="rel-badge">Stable</span>' if pin.get('tag') == r['tag'] else ''
        lis = ''.join(f'<li>{inline(t)} <a class="rel-pr" href="https://github.com/{REPO}/pull/{n}">#{n}</a></li>' for n, t in changes)
        items.append(f'''      <article class="rel" id="{r['tag']}">
        <div class="rel-meta"><h2><a href="https://github.com/{REPO}/releases/tag/{r['tag']}">{r['tag'].lstrip('v')}</a></h2>{badge}<time datetime="{r['date']}">{month}</time></div>
        <div class="rel-body">{f'<p>{inline(summary)}</p>' if summary else ''}{f'<ul>{lis}</ul>' if lis else ''}</div>
      </article>''')
    return f'''<main id="top">
  <section class="page-head wrap">
    <h1>Release notes</h1>
    <p>What changed in each Bowser release. Every build is on <a href="https://github.com/{REPO}/releases">GitHub</a>, and installed copies update themselves to the stable one.</p>
  </section>
  <section class="releases wrap">
{chr(10).join(items)}
  </section>
</main>'''


def fill_download(body):
    """The stable pin's version, size, date and SHA-256 (tools/stable.json is a copy of the
    repository's stable/release.json)."""
    pin = json.load(open(os.path.join(ROOT, 'tools', 'stable.json')))
    rel = {r['tag']: r for r in json.load(open(os.path.join(ROOT, 'tools', 'releases.json')))}
    date = rel.get(pin['tag'], {}).get('date')
    when = __import__('datetime').date.fromisoformat(date).strftime('%-d %B %Y') if date else ''
    return (body.replace('{{version}}', html.escape(pin['version'])).replace('{{sha}}', html.escape(pin['sha256']))
            .replace('{{size}}', f"{pin['bytes'] / 1_000_000:.1f} MB").replace('{{date}}', when))


def build():
    layout = open(os.path.join(ROOT, 'tools', 'layout.html')).read()
    for page, (title, desc, active) in PAGES.items():
        body = releases_body() if page == 'releases' else open(os.path.join(ROOT, 'src', page + '.html')).read()
        if page == 'download':
            body = fill_download(body)
        links = '\n      '.join(
            f'<a href="{key}.html"' + (' aria-current="page"' if key == active else '') + f'>{label}</a>' for key, label in NAV)
        out = (layout.replace('{{title}}', html.escape(title)).replace('{{description}}', html.escape(desc))
               .replace('{{navlinks}}', links).replace('{{body}}', body.rstrip()))
        with open(os.path.join(ROOT, page + '.html'), 'w') as f:
            f.write(out)
    print('built', ', '.join(p + '.html' for p in PAGES))


if __name__ == '__main__':
    if '--refresh-releases' in sys.argv:
        refresh_releases()
    build()
