// Bowser Shield panel: one renderer, hosted three ways (the popup, the options page and the in-page
// overlay). It takes a root node and a send() function, asks the background for state, renders it and
// wires the controls. It only ever sets text with textContent, never markup, because host names and
// rule text come from pages and from the user.
//
// The models at the top (summaryModel, statusModel, droppedModel) and the chord helpers are pure and
// are covered by the Node tests. mount() is the DOM part.
//
// Loaded as a classic script: <script src> in the popup and options pages, and listed in the content
// script entry so the overlay can render it too (see VERIFICATION.md on why the manifest lists it).

const ShieldPanel = (function () {
  "use strict";

  var SITE_LIMIT = 500;
  var RULES_LIMIT_BYTES = 256 * 1024;

  // ---- models (pure) ----------------------------------------------------------------------------

  var TEXT = {
    notHere: "Nothing to filter here",
    off: "Shield is off",
    siteOff: "This site is never filtered",
    network: "Network blocking: active",
    networkUnchecked: "Network blocking: not checked yet",
    dom: "Network blocking unavailable — hiding in the page only",
    noCount: "Blocked count unavailable in this runtime",
    filterSite: "Don't filter this site",
    unfilterSite: "Filter this site",
    siteNeedsName: "A site rule needs a name with two or more parts, such as news.example.",
    siteOffByRule: "An off rule in My rules covers this site. Remove it there to filter the site again.",
    readOnly: "These settings were saved by a newer Bowser Shield, so changes are off."
  };

  function summaryModel(state) {
    var page = state.page && state.page.supported ? state.page : null;
    var model = {
      toggle: { on: !!state.enabled, disabled: !!state.readOnly },
      siteName: page ? page.host : "",
      lines: [],
      status: "",
      siteButton: null,
      greyed: !state.enabled,
      readOnly: !!state.readOnly,
      readOnlyNote: state.readOnly ? TEXT.readOnly : null
    };
    if (!page) {
      model.status = TEXT.notHere;
      return model;
    }

    if (state.engineMode === "dom") {
      model.lines.push({ n: page.hidden || 0, text: "hidden on this site" });
    } else if (state.countsAvailable) {
      model.lines.push({ n: page.blocked || 0, text: "blocked on this site" });
      model.lines.push({ n: (state.session && state.session.blocked) || 0, text: "blocked this session" });
    } else {
      model.lines.push({ n: null, text: TEXT.noCount });
    }

    if (!state.enabled) model.status = TEXT.off;
    else if (page.siteOff) model.status = TEXT.siteOff;
    else if (state.engineMode === "dom") model.status = TEXT.dom;
    else if (state.engineMode === "network") model.status = TEXT.network;
    else model.status = TEXT.networkUnchecked;

    var reason = null;
    if (page.siteOff && page.siteOffRuleOnly) reason = TEXT.siteOffByRule;
    else if (!page.siteOff && !page.canExempt) reason = TEXT.siteNeedsName;
    model.siteButton = {
      label: page.siteOff ? TEXT.unfilterSite : TEXT.filterSite,
      disabled: !!state.readOnly || reason !== null,
      reason: reason
    };
    return model;
  }

  function formatCounts(c) {
    c = c || {};
    return (c.block || 0) + " block, " + (c.allow || 0) + " allow, " + (c.hide || 0) + " hide, " + (c.off || 0) + " off";
  }

  function statusModel(state) {
    var counts = state.counts || {};
    var model = {
      engine: "",
      check: null,
      wording: null,
      rules: ["Starter list: " + formatCounts(counts.starter), "Your rules: " + formatCounts(counts.user)]
    };
    if (state.engineMode === "dom") {
      model.engine = TEXT.dom;
      model.wording =
        "“Hidden” means an element was removed from the page. It does not mean the request was stopped, " +
        "which is why this panel says hidden and not blocked.";
    } else if (state.engineMode === "network") {
      model.engine = "Network blocking: API present, enforcement unverified in this runtime";
      model.check = "To check it, open a page that loads from a host your list blocks and look at the blocked count.";
    } else {
      model.engine = "Network layer not checked yet";
    }
    return model;
  }

  function plural(n, one, many) {
    return n === 1 ? one : many;
  }

  function droppedModel(state) {
    var dropped = state.dropped || [];
    if (dropped.length === 0) return null;
    var first = dropped[0].line;
    var regex = 0;
    var pattern = 0;
    for (var i = 0; i < dropped.length; i++) {
      if (dropped[i].line < first) first = dropped[i].line;
      if (dropped[i].reason === "regex-cap") regex++;
      if (dropped[i].reason === "path-pattern-char") pattern++;
    }
    var n = dropped.length;
    return {
      text: n + plural(n, " rule is", " rules are") + " not being enforced, starting at line " + first + ".",
      regexText:
        regex > 0
          ? regex + " label " + plural(regex, "rule is", "rules are") + " over the regex limit. In network mode there is no fallback, so " +
            plural(regex, "it does", "they do") + " not apply."
          : null,
      patternText:
        pattern > 0
          ? pattern + " path " + plural(pattern, "rule uses", "rules use") + " *, | or ^, which a plain substring match cannot express, so " +
            plural(pattern, "it does", "they do") + " not apply in network mode."
          : null
    };
  }

  // ---- the chord (section 7.1) ------------------------------------------------------------------

  var MODIFIERS = { alt: "alt", shift: "shift", ctrl: "ctrl", control: "ctrl", meta: "meta" };

  function parseChord(text) {
    if (typeof text !== "string") return null;
    var parts = text.split("+");
    if (parts.length < 2) return null;
    var chord = { alt: false, shift: false, ctrl: false, meta: false, code: null };
    for (var i = 0; i < parts.length; i++) {
      var word = parts[i].trim();
      var low = word.toLowerCase();
      if (Object.prototype.hasOwnProperty.call(MODIFIERS, low)) {
        if (chord[MODIFIERS[low]]) return null;
        chord[MODIFIERS[low]] = true;
      } else if (chord.code !== null) {
        return null;
      } else if (/^[a-z]$/i.test(word)) {
        chord.code = "Key" + word.toUpperCase();
      } else if (/^[0-9]$/.test(word)) {
        chord.code = "Digit" + word;
      } else if (/^f([1-9]|1[0-2])$/i.test(word)) {
        chord.code = "F" + word.slice(1);
      } else {
        return null;
      }
    }
    if (chord.code === null) return null;
    if (!chord.alt && !chord.ctrl && !chord.meta) return null;
    return chord;
  }

  function matchChord(chord, event) {
    return (
      !!chord &&
      event.code === chord.code &&
      !!event.altKey === chord.alt &&
      !!event.shiftKey === chord.shift &&
      !!event.ctrlKey === chord.ctrl &&
      !!event.metaKey === chord.meta
    );
  }

  function isEditable(el) {
    if (!el) return false;
    var tag = String(el.tagName || "").toUpperCase();
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
    if (el.isContentEditable) return true;
    if (typeof el.getAttribute === "function") {
      var role = el.getAttribute("role");
      if (role && role.toLowerCase() === "textbox") return true;
    }
    return false;
  }

  // ---- rendering --------------------------------------------------------------------------------

  function h(doc, tag, o, kids) {
    var el = doc.createElement(tag);
    o = o || {};
    if (o.class) el.className = o.class;
    if (o.text !== undefined) el.textContent = o.text;
    var k;
    if (o.attrs) for (k in o.attrs) el.setAttribute(k, o.attrs[k]);
    if (o.props) for (k in o.props) el[k] = o.props[k];
    if (o.on) for (k in o.on) el.addEventListener(k, o.on[k]);
    if (kids) {
      for (var i = 0; i < kids.length; i++) if (kids[i]) el.appendChild(kids[i]);
    }
    return el;
  }

  function withCommas(n) {
    return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  }

  function utf8Bytes(text) {
    if (typeof TextEncoder === "function") return new TextEncoder().encode(text).length;
    return text.length;
  }

  function mount(root, opts) {
    var doc = opts.doc || (typeof document !== "undefined" ? document : null);
    var core = opts.core || (typeof globalThis !== "undefined" ? globalThis.ShieldCore : null);
    var send = opts.send;
    var view = opts.view === "options" ? "options" : "summary";
    var canGoBack = view === "summary";

    var state = null;
    var failure = null;
    var notice = "";
    var draft = null; // unsaved text in the rules editor
    var siteDraft = "";
    var siteError = "";
    var chordDraft = null;
    var chordError = "";

    function act(message) {
      return send(message).then(
        function (next) {
          state = next;
          failure = null;
          if (next && next.error) notice = next.error;
          render();
        },
        function () {
          failure = "Bowser Shield's background worker did not answer. Reload the page and try again.";
          render();
        }
      );
    }

    function activeKey() {
      var node = root.getRootNode ? root.getRootNode() : null;
      var active = (node && node.activeElement) || doc.activeElement;
      return active && active.getAttribute ? active.getAttribute("data-key") : null;
    }

    function find(key) {
      var found = null;
      (function walk(node) {
        var kids = node.children || [];
        for (var i = 0; i < kids.length && !found; i++) {
          if (kids[i].getAttribute && kids[i].getAttribute("data-key") === key) found = kids[i];
          else walk(kids[i]);
        }
      })(root);
      return found;
    }

    function button(label, key, onClick, extra) {
      var o = { class: (extra && extra.class) || "bs-button", text: label, attrs: { type: "button", "data-key": key }, on: { click: onClick } };
      if (extra && extra.disabled) o.props = { disabled: true };
      if (extra && extra.title) o.attrs.title = extra.title;
      return h(doc, "button", o);
    }

    function render() {
      var key = state ? activeKey() : null;
      root.textContent = "";
      var panel = h(doc, "div", { class: "bs-panel", attrs: { "data-view": view } });
      if (failure) panel.appendChild(renderFailure());
      else if (!state) panel.appendChild(h(doc, "p", { class: "bs-muted", text: "Loading…" }));
      else if (view === "options") renderOptions(panel);
      else renderSummary(panel);
      root.appendChild(panel);
      if (key) {
        var again = find(key);
        if (again && again.focus) again.focus();
      }
    }

    function renderFailure() {
      return h(doc, "div", { class: "bs-banner", attrs: { role: "alert" } }, [h(doc, "p", { text: failure })]);
    }

    function closeButton() {
      return opts.onClose ? button("Close", "close", opts.onClose, { class: "bs-button bs-quiet" }) : null;
    }

    // -- summary view -------------------------------------------------------------------------

    function renderSummary(panel) {
      var m = summaryModel(state);
      var sw = h(
        doc,
        "button",
        {
          class: "bs-switch",
          attrs: {
            type: "button",
            role: "switch",
            "aria-checked": m.toggle.on ? "true" : "false",
            "aria-label": "Shield",
            "data-key": "enabled"
          },
          props: m.toggle.disabled ? { disabled: true } : {},
          on: {
            click: function () {
              act({ type: "setEnabled", value: !state.enabled });
            }
          }
        },
        [
          h(doc, "span", { class: "bs-switch-text", text: m.toggle.on ? "On" : "Off" }),
          h(doc, "span", { class: "bs-gate", attrs: { "aria-hidden": "true" } })
        ]
      );
      panel.appendChild(
        h(doc, "div", { class: "bs-head" }, [h(doc, "h1", { class: "bs-title", text: "Bowser Shield" }), closeButton(), sw])
      );
      panel.appendChild(h(doc, "p", { class: "bs-hint", text: "The switch applies to every site." }));
      if (m.readOnlyNote) panel.appendChild(h(doc, "p", { class: "bs-warn", text: m.readOnlyNote }));
      if (m.siteName) panel.appendChild(h(doc, "p", { class: "bs-site", text: m.siteName }));

      if (m.lines.length > 0) {
        var counts = h(doc, "div", { class: m.greyed ? "bs-counts bs-greyed" : "bs-counts" });
        m.lines.forEach(function (line) {
          if (line.n === null) counts.appendChild(h(doc, "p", { class: "bs-count bs-count-plain", text: line.text }));
          else {
            counts.appendChild(
              h(doc, "p", { class: "bs-count" }, [
                h(doc, "span", { class: "bs-n", text: String(line.n) }),
                h(doc, "span", { class: "bs-count-text", text: line.text })
              ])
            );
          }
        });
        panel.appendChild(counts);
      }
      panel.appendChild(h(doc, "p", { class: "bs-status", text: m.status }));

      var actions = h(doc, "div", { class: "bs-actions" });
      if (m.siteButton) {
        var host = state.page.host;
        actions.appendChild(
          button(
            m.siteButton.label,
            "site-toggle",
            function () {
              var off = !state.page.siteOff;
              notice = "Takes effect the next time " + host + " loads.";
              act({ type: "setSiteOff", host: host, off: off });
            },
            { disabled: m.siteButton.disabled, title: m.siteButton.reason || undefined }
          )
        );
        if (m.siteButton.reason) actions.appendChild(h(doc, "p", { class: "bs-hint", text: m.siteButton.reason }));
      }
      actions.appendChild(
        button("Settings…", "settings", function () {
          view = "options";
          notice = "";
          render();
        })
      );
      panel.appendChild(actions);
      panel.appendChild(h(doc, "p", { class: "bs-notice", attrs: { "data-key": "notice", "aria-live": "polite" }, text: notice }));
      panel.appendChild(h(doc, "p", { class: "bs-hint", text: "Open this panel on any page with " + (state.chord || "Alt+Shift+S") + "." }));
    }

    // -- options view -------------------------------------------------------------------------

    function renderOptions(panel) {
      var head = h(doc, "div", { class: "bs-head" }, [
        h(doc, "h1", { class: "bs-title", text: canGoBack ? "Bowser Shield settings" : "Bowser Shield" }),
        canGoBack
          ? button("Back", "back", function () {
              view = "summary";
              notice = "";
              render();
            }, { class: "bs-button bs-quiet" })
          : null,
        closeButton()
      ]);
      panel.appendChild(head);
      if (state.readOnly) panel.appendChild(h(doc, "p", { class: "bs-warn", text: TEXT.readOnly }));
      if (notice) panel.appendChild(h(doc, "p", { class: "bs-notice", attrs: { "data-key": "notice", "aria-live": "polite" }, text: notice }));

      panel.appendChild(renderStatusSection());
      var banner = droppedModel(state);
      if (banner) panel.appendChild(renderBanner(banner));
      panel.appendChild(renderRulesSection());
      panel.appendChild(renderSitesSection());
      panel.appendChild(renderStarterSection());
    }

    function section(title, kids) {
      return h(doc, "section", { class: "bs-section" }, [h(doc, "h2", { class: "bs-h2", text: title })].concat(kids));
    }

    function renderStatusSection() {
      var s = statusModel(state);
      var kids = [h(doc, "p", { class: "bs-line", attrs: { "data-key": "engine" }, text: s.engine })];
      if (s.check) kids.push(h(doc, "p", { class: "bs-hint", text: s.check }));
      if (s.wording) kids.push(h(doc, "p", { class: "bs-hint", text: s.wording }));
      s.rules.forEach(function (line) {
        kids.push(h(doc, "p", { class: "bs-line", text: line }));
      });
      if (state.badSelectors > 0) {
        var n = state.badSelectors;
        kids.push(
          h(doc, "p", {
            class: "bs-line",
            text: n + plural(n, " selector was", " selectors were") + " skipped on this page because the browser rejected " + plural(n, "it.", "them.")
          })
        );
      }
      kids.push(button("Re-check", "recheck", function () { act({ type: "recheck" }); }));

      var chordInput = h(doc, "input", {
        class: "bs-input",
        attrs: { type: "text", "data-key": "chord", "aria-label": "Panel shortcut", spellcheck: "false", autocomplete: "off" },
        props: { value: chordDraft !== null ? chordDraft : state.chord || "Alt+Shift+S" },
        on: {
          input: function (e) {
            chordDraft = e.target.value;
          }
        }
      });
      kids.push(
        h(doc, "div", { class: "bs-row" }, [
          h(doc, "label", { class: "bs-label", text: "Panel shortcut" }, []),
          chordInput,
          button("Save shortcut", "chord-save", function () {
            var value = chordInput.value.trim();
            if (!parseChord(value)) {
              chordDraft = value;
              chordError = "Use a modifier and a letter, digit or F-key, such as Alt+Shift+S.";
              render();
              return;
            }
            chordDraft = null;
            chordError = "";
            act({ type: "setChord", value: value });
          }, { disabled: state.readOnly })
        ])
      );
      kids.push(h(doc, "p", { class: "bs-error", attrs: { "data-key": "chord-error", role: "alert" }, text: chordError }));
      return section("Status", kids);
    }

    function renderBanner(b) {
      var kids = [h(doc, "p", { text: b.text })];
      if (b.regexText) kids.push(h(doc, "p", { text: b.regexText }));
      if (b.patternText) kids.push(h(doc, "p", { text: b.patternText }));
      return h(doc, "div", { class: "bs-banner", attrs: { role: "alert", "data-key": "dropped" } }, kids);
    }

    function lineRange(text, lineNo) {
      var lines = text.split("\n");
      var start = 0;
      for (var i = 0; i < lineNo - 1 && i < lines.length; i++) start += lines[i].length + 1;
      var line = lines[lineNo - 1] || "";
      return [start, start + line.replace(/\r$/, "").length];
    }

    function renderRulesSection() {
      var text = draft !== null ? draft : state.userRules || "";
      var area = h(doc, "textarea", {
        class: "bs-editor",
        attrs: { "data-key": "rules", "aria-label": "Rules, one per line", spellcheck: "false", rows: "9", wrap: "off" },
        props: { value: text, readOnly: !!state.readOnly },
        on: {
          input: function (e) {
            draft = e.target.value;
            refreshFeedback();
          }
        }
      });
      var counter = h(doc, "p", { class: "bs-hint", attrs: { "data-key": "rules-count" } });
      var errors = h(doc, "ul", { class: "bs-errors", attrs: { "data-key": "errors" } });

      function refreshFeedback() {
        var current = area.value;
        var bytes = utf8Bytes(current);
        counter.textContent = withCommas(bytes) + " of " + withCommas(RULES_LIMIT_BYTES) + " characters" + (bytes > RULES_LIMIT_BYTES ? " (over the limit)" : "");
        errors.textContent = "";
        var compiled = core.compile(current);
        compiled.errors.forEach(function (err) {
          var item = h(doc, "li", {}, [
            h(doc, "button", {
              class: "bs-linkish",
              text: "line " + err.line + ": " + err.message,
              attrs: { type: "button" },
              on: {
                click: function () {
                  var range = lineRange(area.value, err.line);
                  area.focus();
                  if (area.setSelectionRange) area.setSelectionRange(range[0], range[1]);
                }
              }
            })
          ]);
          errors.appendChild(item);
        });
      }

      var save = button("Save", "save-rules", function () {
        var value = area.value;
        draft = null;
        notice = "Saved.";
        act({ type: "saveRules", text: value });
      }, { disabled: state.readOnly });

      var kids = [area, counter, h(doc, "div", { class: "bs-actions" }, [save]), errors, formatHelp()];
      var sec = section("My rules", kids);
      refreshFeedback();
      return sec;
    }

    function formatHelp() {
      function table(head, rows) {
        return h(doc, "table", { class: "bs-table" }, [
          h(doc, "thead", {}, [h(doc, "tr", {}, head.map(function (c) { return h(doc, "th", { text: c, attrs: { scope: "col" } }); }))]),
          h(doc, "tbody", {}, rows.map(function (r) { return h(doc, "tr", {}, r.map(function (c) { return h(doc, "td", { text: c }); })); }))
        ]);
      }
      return h(doc, "details", { class: "bs-help" }, [
        h(doc, "summary", { text: "Format help" }),
        h(doc, "p", {
          text: "One rule per line. Lines starting with # are comments. Words are lowercase and separated by spaces, not tabs."
        }),
        table(
          ["Verb", "Takes", "Qualifiers", "Meaning"],
          [
            ["block", "host, label, path", "kind, party, only, except", "stop the matching request"],
            ["allow", "host, label, path", "kind, party, only, except", "never block the matching request"],
            ["hide", "css", "only, except", "hide matching elements"],
            ["off", "site", "none", "never filter this site"]
          ]
        ),
        table(
          ["Kind", "Value"],
          [
            ["host", "two or more labels, a-z 0-9 and -. Matches the host and its subdomains."],
            ["site", "the same as host. Names the page being visited."],
            ["label", "4 to 40 characters, a-z 0-9 and -, with a letter. Matches inside one label of the host."],
            ["path", "starts with /, 3 to 200 characters, no space, #, \" or \\. Not case-sensitive."],
            ["css", "3 to 200 characters, no spaces or commas, and one of #, . or [."]
          ]
        ),
        h(doc, "p", {
          text: "block and allow apply to third-party requests only. Write party any or party first to change that. kind takes script, image, frame, xhr, media, font, style, ping and other."
        }),
        h(doc, "p", { text: "only and except take up to 16 hosts, separated by commas. except is checked first. off beats allow, and allow beats block." }),
        h(doc, "p", { text: "Write non-ASCII host names in punycode (xn--…). A line with a mistake is skipped and listed below; the other lines still apply." })
      ]);
    }

    function renderSitesSection() {
      var list = state.siteOff || [];
      var input = h(doc, "input", {
        class: "bs-input",
        attrs: { type: "text", "data-key": "site-input", "aria-label": "Site to never filter", spellcheck: "false", autocomplete: "off", placeholder: "news.example" },
        props: { value: siteDraft },
        on: {
          input: function (e) {
            siteDraft = e.target.value;
          }
        }
      });
      var add = button("Add", "site-add", function () {
        var value = input.value.trim().toLowerCase();
        siteDraft = value;
        if (!core.isValidSite(value)) siteError = "A site needs two or more parts, such as news.example, using a-z, 0-9 and -.";
        else if (list.indexOf(value) >= 0) siteError = value + " is already in the list.";
        else if (list.length >= SITE_LIMIT) siteError = "The list is full at " + SITE_LIMIT + " sites. Remove one to add another.";
        else {
          siteError = "";
          siteDraft = "";
          act({ type: "saveSiteOff", list: list.concat([value]) });
          return;
        }
        render();
      }, { disabled: state.readOnly });

      var items = list.map(function (host) {
        return h(doc, "li", { class: "bs-site-row" }, [
          h(doc, "span", { text: host }),
          button("Remove", "site-remove:" + host, function () {
            act({ type: "saveSiteOff", list: list.filter(function (x) { return x !== host; }) });
          }, { class: "bs-button bs-quiet", disabled: state.readOnly, title: "Remove " + host })
        ]);
      });
      var kids = [
        h(doc, "p", { class: "bs-hint", text: "Nothing is filtered on these sites or their subdomains. Changes apply the next time a site loads." }),
        h(doc, "div", { class: "bs-row" }, [input, add]),
        h(doc, "p", { class: "bs-error", attrs: { "data-key": "site-error", role: "alert" }, text: siteError }),
        list.length > 0
          ? h(doc, "ul", { class: "bs-sites" }, items)
          : h(doc, "p", { class: "bs-muted", text: "No sites yet. Add one you never want filtered." }),
        h(doc, "p", { class: "bs-hint", text: list.length + " of " + SITE_LIMIT })
      ];
      return section("Sites never filtered", kids);
    }

    function renderStarterSection() {
      var box = h(doc, "input", {
        attrs: { type: "checkbox", id: "bs-starter", "data-key": "starter" },
        props: { checked: !!state.starterEnabled, disabled: !!state.readOnly },
        on: {
          change: function (e) {
            act({ type: "setStarterEnabled", value: !!e.target.checked });
          }
        }
      });
      var listing = h(doc, "textarea", {
        class: "bs-editor bs-readonly",
        text: state.starterSrl || "",
        attrs: { "data-key": "starter-text", "aria-label": "Built-in starter list, read-only", readonly: "", rows: "9", wrap: "off", spellcheck: "false" },
        props: { readOnly: true, value: state.starterSrl || "" }
      });
      return section("Built-in starter list", [
        h(doc, "div", { class: "bs-row" }, [box, h(doc, "label", { text: "Use the built-in starter list", attrs: { for: "bs-starter" } })]),
        h(doc, "p", { class: "bs-hint", text: "Every rule you are trusting is listed here. Edit your own rules above; this list is read-only." }),
        listing
      ]);
    }

    render();
    return send({ type: "getState" }).then(
      function (next) {
        state = next;
        render();
        return { refresh: function () { return act({ type: "getState" }); } };
      },
      function () {
        failure = "Bowser Shield's background worker did not answer. Reload the page and try again.";
        render();
        return { refresh: function () { return act({ type: "getState" }); } };
      }
    );
  }

  return {
    summaryModel: summaryModel,
    statusModel: statusModel,
    droppedModel: droppedModel,
    parseChord: parseChord,
    matchChord: matchChord,
    isEditable: isEditable,
    mount: mount
  };
})();

if (typeof module === "object" && module.exports) module.exports = ShieldPanel; // Node tests
if (typeof self === "object") self.ShieldPanel = self.ShieldPanel || ShieldPanel; // content script
if (typeof window === "object") window.ShieldPanel = ShieldPanel; // popup/options
