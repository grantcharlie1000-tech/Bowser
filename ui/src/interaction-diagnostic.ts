// Passive, opt-in synthetic-profile event ordering. Never records input text or URLs.
export type InteractionEvent = 'armed' | 'enter_capture' | 'enter_bubble' | 'focus_in' | 'focus_out';
const installed = new WeakSet<HTMLInputElement>();
export function interactionFlags(input: HTMLInputElement, event?: KeyboardEvent): number {
  const doc = input.ownerDocument;
  return Number(event?.isComposing === true)
    | (Number(event?.ctrlKey === true) << 1) | (Number(event?.shiftKey === true) << 2)
    | (Number(event?.altKey === true) << 3) | (Number(event?.metaKey === true) << 4)
    | (Number(doc.activeElement === input) << 5) | (Number(doc.hasFocus()) << 6)
    | (Number(event?.defaultPrevented === true) << 7)
    | (Number(input.value === 'bowser://settings') << 8)
    | (Number(input.selectionStart === 0 && input.selectionEnd === input.value.length) << 9)
    | (Number(input.value.length === 0) << 10) | (Number(event?.keyCode === 229) << 11);
}
export function installInteractionDiagnostic(input: HTMLInputElement, report: (event: InteractionEvent, flags: number) => void, state: () => number = () => 0): void {
  if (installed.has(input)) return;
  installed.add(input);
  const flags = (event?: KeyboardEvent): number => interactionFlags(input, event) | (state() & 0xf000);
  // No preventDefault, propagation, value, selection or focus mutation. Telemetry uses a separate bounded
  // diagnostic-only intake after unchanged trusted-origin checks; only Enter/focus transitions are sent, never each edit.
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') report('enter_capture', flags(event));
  }, { capture: true, passive: true });
  input.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') report('enter_bubble', flags(event));
  }, { passive: true });
  input.addEventListener('focus', () => { report('focus_in', flags()); }, { passive: true });
  input.addEventListener('blur', () => { report('focus_out', flags()); }, { passive: true });
  report('armed', flags());
}
