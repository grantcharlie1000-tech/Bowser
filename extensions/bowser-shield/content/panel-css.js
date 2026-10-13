// The overlay panel's stylesheet, as a string. It is a script rather than a .css file so that no
// extension file has to be exposed to pages through web_accessible_resources.
//
// This is the same block as the one between "panel:begin" and "panel:end" in ui/ui.css; a test keeps
// the two identical. Edit them together.

self.SHIELD_PANEL_CSS = `
.bs-overlay,
.bs-panel {
  --bs-ink: #1d2733;
  --bs-paper: #f2f4f6;
  --bs-slate: #566676;
  --bs-rule: #cbd3da;
  --bs-gate: #1c6a7a;
  --bs-alert: #b3261e;
  --bs-font: "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
}

.bs-overlay {
  box-sizing: border-box;
  width: min(320px, calc(100vw - 24px));
  max-height: calc(100vh - 24px);
  overflow: auto;
  background: var(--bs-paper);
  border: 1px solid var(--bs-rule);
  border-radius: 6px;
  box-shadow: 0 8px 28px rgba(29, 39, 51, 0.22);
}

.bs-panel {
  box-sizing: border-box;
  width: 100%;
  padding: 14px 16px 16px;
  color: var(--bs-ink);
  background: var(--bs-paper);
  color-scheme: light;
  font: 13px/1.45 var(--bs-font);
  font-variant-numeric: tabular-nums;
  text-align: left;
}

.bs-panel *,
.bs-panel *::before,
.bs-panel *::after {
  box-sizing: border-box;
}

.bs-panel h1,
.bs-panel h2,
.bs-panel p,
.bs-panel ul {
  margin: 0;
  padding: 0;
}

.bs-panel ul {
  list-style: none;
}

.bs-panel [hidden] {
  display: none !important;
}

.bs-panel :focus-visible {
  outline: 2px solid var(--bs-gate);
  outline-offset: 2px;
}

.bs-head {
  display: flex;
  align-items: center;
  gap: 8px;
}

.bs-title {
  flex: 1 1 auto;
  font-size: 15px;
  font-weight: 600;
  line-height: 1.3;
}

.bs-switch {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 3px 4px 3px 10px;
  font: inherit;
  font-weight: 600;
  color: var(--bs-slate);
  background: transparent;
  border: 1px solid var(--bs-slate);
  border-radius: 3px;
  cursor: pointer;
}

.bs-switch:disabled {
  cursor: default;
  border-style: dashed;
}

.bs-switch[aria-checked="true"] {
  color: var(--bs-paper);
  background: var(--bs-gate);
  border-color: var(--bs-gate);
}

.bs-gate {
  position: relative;
  display: inline-block;
  width: 30px;
  height: 18px;
  overflow: hidden;
  border: 1px solid currentColor;
  border-radius: 2px;
}

.bs-gate::before {
  content: "";
  position: absolute;
  inset: 0;
  background: repeating-linear-gradient(90deg, currentColor 0 2px, transparent 2px 6px);
  transform-origin: top;
  transform: scaleY(0.2);
}

.bs-switch[aria-checked="true"] .bs-gate::before {
  transform: scaleY(1);
}

.bs-hint {
  margin-top: 4px;
  color: var(--bs-slate);
  font-size: 12px;
}

.bs-muted {
  color: var(--bs-slate);
}

.bs-warn {
  margin-top: 8px;
  color: var(--bs-alert);
}

.bs-site {
  margin-top: 12px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.bs-counts {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--bs-rule);
}

.bs-greyed {
  color: var(--bs-slate);
}

.bs-count {
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.bs-n {
  min-width: 3ch;
  font-size: 26px;
  font-weight: 600;
  line-height: 1.15;
}

.bs-count-plain {
  color: var(--bs-slate);
}

.bs-status {
  margin-top: 8px;
  font-weight: 600;
}

.bs-actions {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  margin-top: 12px;
}

.bs-button {
  padding: 6px 10px;
  font: inherit;
  color: var(--bs-ink);
  text-align: left;
  background: transparent;
  border: 1px solid var(--bs-rule);
  border-radius: 3px;
  cursor: pointer;
}

.bs-button:hover:not(:disabled) {
  border-color: var(--bs-ink);
}

.bs-button:disabled {
  color: var(--bs-slate);
  border-style: dashed;
  cursor: default;
}

.bs-quiet {
  padding-inline: 6px;
  border-color: transparent;
}

.bs-notice {
  margin-top: 8px;
  color: var(--bs-gate);
  font-weight: 600;
}

.bs-notice:empty,
.bs-error:empty {
  display: none;
}

.bs-error {
  color: var(--bs-alert);
}

.bs-banner {
  margin-top: 14px;
  padding: 8px 10px;
  border-left: 3px solid var(--bs-alert);
}

.bs-banner p + p {
  margin-top: 4px;
}

.bs-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 18px;
  padding-top: 12px;
  border-top: 1px solid var(--bs-rule);
}

.bs-section > .bs-button {
  align-self: flex-start;
}

.bs-h2 {
  font-size: 13px;
  font-weight: 600;
}

.bs-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.bs-label {
  flex: 0 0 100%;
  color: var(--bs-slate);
  font-size: 12px;
}

.bs-input {
  flex: 1 1 120px;
  min-width: 0;
  padding: 5px 8px;
  font: inherit;
  color: inherit;
  background: transparent;
  border: 1px solid var(--bs-rule);
  border-radius: 3px;
}

.bs-editor {
  width: 100%;
  min-height: 9em;
  padding: 6px 8px;
  font: inherit;
  font-size: 12.5px;
  line-height: 1.5;
  color: inherit;
  white-space: pre;
  overflow: auto;
  background: transparent;
  border: 1px solid var(--bs-rule);
  border-radius: 3px;
  resize: vertical;
}

.bs-readonly {
  color: var(--bs-slate);
}

.bs-errors {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.bs-linkish {
  padding: 2px 0;
  font: inherit;
  color: var(--bs-alert);
  text-align: left;
  text-decoration: underline;
  background: none;
  border: 0;
  cursor: pointer;
}

.bs-line {
  overflow-wrap: anywhere;
}

.bs-help summary {
  cursor: pointer;
}

.bs-help p {
  margin-top: 6px;
}

.bs-table {
  width: 100%;
  margin-top: 6px;
  font-size: 12px;
  border-collapse: collapse;
}

.bs-table th,
.bs-table td {
  padding: 3px 6px 3px 0;
  text-align: left;
  vertical-align: top;
  border-bottom: 1px solid var(--bs-rule);
}

.bs-table th {
  font-weight: 600;
}

.bs-site-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 2px 0;
  border-bottom: 1px solid var(--bs-rule);
  overflow-wrap: anywhere;
}

@media (prefers-reduced-motion: no-preference) {
  .bs-gate::before {
    transition: transform 180ms ease;
  }

  .bs-overlay {
    animation: bs-in 140ms ease-out;
  }

  @keyframes bs-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }

    to {
      opacity: 1;
      transform: none;
    }
  }
}
`;
