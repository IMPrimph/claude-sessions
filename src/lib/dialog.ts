// A real modal: the browser moves focus in, keeps Tab inside, makes the rest of the page
// inert, closes on Esc and puts focus back. Every way of closing goes through dialog.close(),
// so all of that also applies to the close button and a click on the backdrop.
export function modal(dialog: HTMLDialogElement, onClose: () => void) {
  dialog.addEventListener("close", onClose);
  dialog.addEventListener("click", (event) => {
    if (event.target === dialog) dialog.close();
  });
  dialog.addEventListener("keydown", (event) => {
    if (event.key !== "Tab") return;
    // Left alone, Tab passes through the empty page between the last control and the first.
    const stops = dialog.querySelectorAll<HTMLElement>('button:not([disabled]), input:checked, [tabindex="0"]');
    const edge = event.shiftKey ? stops[0] : stops[stops.length - 1];
    if (document.activeElement !== edge) return;
    event.preventDefault();
    (event.shiftKey ? stops[stops.length - 1] : stops[0])?.focus();
  });
  dialog.showModal();
}

export function closeDialog(event: Event) {
  (event.currentTarget as Element).closest("dialog")?.close();
}

// Window-level shortcuts still receive keys while a modal is open, so they check this first.
export function dialogIsOpen(): boolean {
  return document.querySelector("dialog[open]") !== null;
}
