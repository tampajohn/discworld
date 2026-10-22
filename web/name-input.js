// Browser text entry is only a keyboard endpoint; original-font rendering and
// length/width validation stay in the engine. A sentinel enables backspace even
// when this endpoint has already forwarded its previous text.
export function nameInput(element, send) {
  const sentinel = '\u200b';
  let committedComposition = null;
  const reset = () => { element.value = sentinel; element.setSelectionRange(1, 1); };
  const insert = text => {
    const ascii = text.replace(/[^\x20-\x7e]/g, '').slice(0, 80);
    if (ascii) send(ascii, false);
  };
  const repeatedComposition = event => committedComposition !== null
    && event.inputType?.toLowerCase().includes('composition') && event.data === committedComposition;
  element.addEventListener('compositionstart', () => { committedComposition = null; });
  element.addEventListener('beforeinput', event => {
    if (repeatedComposition(event) && event.cancelable) {
      event.preventDefault(); committedComposition = null; reset(); return;
    }
    if (event.isComposing || !event.cancelable) return;
    if (event.inputType === 'deleteContentBackward') {
      event.preventDefault(); send('', true); reset();
    } else if (event.inputType.startsWith('insert') && event.data !== null) {
      event.preventDefault(); insert(event.data); reset();
    }
  });
  element.addEventListener('input', event => {
    if (event.isComposing) return;
    if (repeatedComposition(event)) { committedComposition = null; reset(); return; }
    if (event.inputType === 'deleteContentBackward') send('', true);
    else insert(element.value);
    reset();
  });
  element.addEventListener('compositionend', () => {
    committedComposition = element.value.replaceAll(sentinel, '');
    insert(committedComposition); reset();
  });
  element.addEventListener('keydown', event => {
    if (event.key === 'Enter' || event.key === 'Escape') {
      event.preventDefault(); element.blur();
    }
  });
  reset();
  return () => { reset(); element.focus({ preventScroll: true }); };
}
