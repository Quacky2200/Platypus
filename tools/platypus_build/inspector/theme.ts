// The UI inspector's look: one stylesheet with colour variables for a dark and a light theme, the field,
// pill and popup styles, added to the UI once (`uiInspEnsureCss`). The theme follows the system
// (`prefers-color-scheme`) unless an entry point puts `uii-dark` or `uii-light` next to `uii` on its root.
// Names start with `uiInsp` / `UiInsp`. When the build appends this file it drops the `export`s.
// Design: docs/research/platypus-ui-inspector.md

const UI_INSP_DARK = `
  --uii-bg: #161a22;
  --uii-bg2: #1e2430;
  --uii-text: #dfe7f5;
  --uii-dim: #8a94a3;
  --uii-line: rgba(255, 255, 255, 0.12);
  --uii-hover: rgba(255, 255, 255, 0.07);
  --uii-field: rgba(255, 255, 255, 0.06);
  --uii-accent: #6cb6ff;
  --uii-accent-bg: rgba(108, 182, 255, 0.22);
  --uii-prop: #9cc4ff;
  --uii-sel: #f0b86e;
  --uii-warn: #e07a7a;
  --uii-shadow: rgba(0, 0, 0, 0.5);
`;

const UI_INSP_LIGHT = `
  --uii-bg: #fbfbfd;
  --uii-bg2: #f0f2f6;
  --uii-text: #1c2330;
  --uii-dim: #6b7685;
  --uii-line: rgba(20, 30, 50, 0.16);
  --uii-hover: rgba(20, 30, 50, 0.07);
  --uii-field: rgba(20, 30, 50, 0.05);
  --uii-accent: #1a6ee0;
  --uii-accent-bg: rgba(26, 110, 224, 0.2);
  --uii-prop: #1d5bb8;
  --uii-sel: #a8531b;
  --uii-warn: #c43a3a;
  --uii-shadow: rgba(20, 30, 50, 0.25);
`;

export const UI_INSP_CSS = `
.uii { ${UI_INSP_DARK} }
@media (prefers-color-scheme: light) { .uii { ${UI_INSP_LIGHT} } }
.uii.uii-dark { ${UI_INSP_DARK} }
.uii.uii-light { ${UI_INSP_LIGHT} }

.uii-card {
  background-color: var(--uii-bg);
  border: 1px solid var(--uii-line);
  border-radius: 10px;
  box-shadow: 0 10px 32px var(--uii-shadow);
  color: var(--uii-text);
}
.uii-head { flex-direction: row; align-items: center; gap: 8px; padding-bottom: 6px; margin-bottom: 6px; border-bottom: 1px solid var(--uii-line); }
.uii-title { color: var(--uii-accent); font-size: 13px; }
.uii-section { color: var(--uii-dim); font-size: 10px; letter-spacing: 0.08em; margin-top: 6px; margin-bottom: 2px; }
.uii-text { color: var(--uii-text); font-size: 12px; }
.uii-dim { color: var(--uii-dim); font-size: 11px; }
.uii-sel { color: var(--uii-sel); font-size: 12px; }
.uii-prop { color: var(--uii-prop); font-size: 12px; padding-top: 2px; }
.uii-off { color: var(--uii-dim); text-decoration: line-through; }
.uii-row { flex-direction: row; flex-wrap: wrap; align-items: flex-start; gap: 4px; padding-left: 18px; border-radius: 4px; }
.uii-block { flex-direction: column; margin-bottom: 6px; }
.uii-line-text { padding-left: 18px; }
.uii-row:hover { background-color: var(--uii-hover); }
.uii-computed { color: var(--uii-sel); font-size: 11px; padding-top: 3px; }

/* A value reads as plain text; editing wraps it snugly: 2px above and below (1px padding, 1px border), 5px at the sides. */
.uii-field {
  padding: 1px 5px;
  font-size: 12px;
  color: var(--uii-text);
  background-color: transparent;
  border: 1px solid transparent;
  border-radius: 4px;
  transition: background-color 0.12s, border-color 0.12s, box-shadow 0.12s;
}
.uii-field:hover { background-color: var(--uii-field); }
.uii-field:focus { background-color: var(--uii-field); border-color: var(--uii-accent); box-shadow: 0 0 0 2px var(--uii-accent-bg); }
.uii-boxed { background-color: var(--uii-field); border-color: var(--uii-line); }
.uii-pill { border-radius: 11px; padding: 1px 9px; min-width: 84px; background-color: var(--uii-field); border-color: var(--uii-accent); box-shadow: 0 0 0 2px var(--uii-accent-bg); }

/* Text that acts: classes and states are links (hand cursor, underline on hover), not buttons. */
.uii-link { color: var(--uii-text); font-size: 12px; cursor: pointer; padding: 1px 2px; border-radius: 3px; }
.uii-link:hover { text-decoration: underline; background-color: var(--uii-hover); }
.uii-link.on { color: var(--uii-accent); text-decoration: underline; }
.uii-link.gone { color: var(--uii-dim); text-decoration: line-through; }
.uii-link.add { color: var(--uii-dim); }
.uii-brace { color: var(--uii-dim); font-size: 12px; }
.uii-group { color: var(--uii-dim); font-size: 11px; margin-top: 6px; }

.uii-chip {
  padding: 1px 8px;
  border-radius: 10px;
  background-color: var(--uii-field);
  border: 1px solid var(--uii-line);
  cursor: pointer;
  transition: background-color 0.12s, border-color 0.12s;
}
.uii-chip:hover { background-color: var(--uii-hover); border-color: var(--uii-dim); }
.uii-chip.on { background-color: var(--uii-accent-bg); border-color: var(--uii-accent); }
.uii-chip-text { color: var(--uii-text); font-size: 11px; }
.uii-chip-dim { color: var(--uii-dim); }

.uii-x { cursor: pointer; padding: 0 5px; border-radius: 4px; }
.uii-x:hover { background-color: var(--uii-hover); }
.uii-x-text { color: var(--uii-warn); font-size: 13px; }

.uii-swatch { border: 1px solid var(--uii-line); border-radius: 4px; cursor: pointer; transition: border-color 0.12s; }
.uii-swatch:hover { border-color: var(--uii-accent); }

.uii-pop {
  background-color: var(--uii-bg2);
  border: 1px solid var(--uii-line);
  border-radius: 8px;
  padding: 8px;
  margin-left: 12px;
  margin-top: 3px;
  margin-bottom: 3px;
  gap: 7px;
  box-shadow: 0 4px 16px var(--uii-shadow);
}
.uii-graph { background-color: var(--uii-field); border: 1px solid var(--uii-line); border-radius: 4px; }
.uii-line { background-color: var(--uii-line); }
.uii-knob { border: 2px solid #ffffff; box-shadow: 0 1px 4px rgba(0, 0, 0, 0.6); }

.uii-sugg {
  margin-top: 2px;
  min-width: 120px;
  max-width: 240px;
  background-color: var(--uii-bg2);
  border: 1px solid var(--uii-line);
  border-radius: 6px;
  padding: 2px;
  box-shadow: 0 6px 18px var(--uii-shadow);
}
.uii-sugg-item { padding: 2px 8px; border-radius: 4px; cursor: pointer; }
.uii-sugg-item:hover { background-color: var(--uii-hover); }
.uii-sugg-item.first { background-color: var(--uii-accent-bg); }

.uii-tip {
  background-color: var(--uii-bg2);
  border: 1px solid var(--uii-line);
  border-radius: 6px;
  padding: 3px 8px;
  box-shadow: 0 4px 14px var(--uii-shadow);
}
/* Marching ants: a selection drawn as moving dashes, never as a fill, so it hides no colour. Four strips
   with a repeating gradient whose position the animation walks by one period. */
@keyframes uii-march-x { from { background-position: 0px 0px; } to { background-position: 6px 0px; } }
@keyframes uii-march-y { from { background-position: 0px 0px; } to { background-position: 0px 6px; } }
.uii-ant-h {
  height: 1px;
  background-image: repeating-linear-gradient(90deg, #ffffff 0px, #ffffff 3px, #101010 3px, #101010 6px);
  animation: uii-march-x 0.4s linear infinite;
}
.uii-ant-v {
  width: 1px;
  background-image: repeating-linear-gradient(180deg, #ffffff 0px, #ffffff 3px, #101010 3px, #101010 6px);
  animation: uii-march-y 0.4s linear infinite;
}
.uii-ant-back { animation-direction: reverse; }

/* The box model drawn over a hovered element, as browsers' devtools do. */
.uii-bm-margin { background-color: rgba(246, 178, 107, 0.45); }
.uii-bm-border { background-color: rgba(255, 229, 153, 0.5); }
.uii-bm-padding { background-color: rgba(147, 196, 125, 0.5); }
.uii-bm-content { background-color: rgba(111, 168, 220, 0.5); }

.uii-flex-box { border: 1px dashed var(--uii-dim); border-radius: 4px; background-color: var(--uii-field); }
`;

let uiInspCssId = 0;

/** Adds the inspector's stylesheet to the UI once. */
export function uiInspEnsureCss(): void {
  if (uiInspCssId === 0) uiInspCssId = addStyleSheet(UI_INSP_CSS);
}
