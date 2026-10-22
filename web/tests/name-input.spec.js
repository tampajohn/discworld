import { test, expect } from '@playwright/test';

test('text endpoint commits composition once and handles input fallback and deletion', async ({ page }) => {
  await page.goto('/');
  const edits = await page.evaluate(async () => {
    const { nameInput } = await import('/name-input.js');
    const input = document.createElement('input'); document.body.append(input);
    const edits = [];
    nameInput(input, (text, backspace) => edits.push({ text, backspace }))();
    input.dispatchEvent(new CompositionEvent('compositionstart'));
    input.value += 'ABC';
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertCompositionText', data: 'ABC', isComposing: true }));
    if (edits.length) throw new Error('Uncommitted composition reached the engine');
    input.dispatchEvent(new CompositionEvent('compositionend', { data: 'ABC' }));
    const final = new InputEvent('beforeinput', { inputType: 'insertCompositionText', data: 'ABC', cancelable: true });
    input.dispatchEvent(final);
    if (!final.defaultPrevented) throw new Error('Duplicate composition was not consumed');
    input.dispatchEvent(new InputEvent('beforeinput', { inputType: 'deleteContentBackward', cancelable: true }));
    // Some paste/input methods provide no cancellable beforeinput event.
    input.value += 'XYZ\n\u{1f600}';
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertFromPaste' }));
    return edits;
  });
  expect(edits).toEqual([
    { text: 'ABC', backspace: false },
    { text: '', backspace: true },
    { text: 'XYZ', backspace: false },
  ]);
});
