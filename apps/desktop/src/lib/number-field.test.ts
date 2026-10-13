import { describe, expect, it, vi } from 'vitest';
import { commitNumber } from './number-field';

/** A number field showing `typed`, as when a person commits it. */
function typedField(typed: string): HTMLInputElement {
  const field = document.createElement('input');
  field.value = typed;
  return field;
}

/** Whole numbers from 1 to 10; nothing for an empty field. */
function oneToTen(typed: string): number | null {
  return typed === '' ? null : Math.min(10, Math.max(1, Math.round(Number(typed))));
}

describe('committing a number field', () => {
  it('saves a new value and shows it as kept', () => {
    const field = typedField('4.6');
    const save = vi.fn();
    commitNumber(field, { read: oneToTen, current: 2, show: String, save });
    expect(save).toHaveBeenCalledWith(5);
    expect(field.value).toBe('5');
  });

  it('shows the saved value when what was typed reads as it, without saving', () => {
    for (const typed of ['0', '-3', '1.2']) {
      const field = typedField(typed);
      const save = vi.fn();
      commitNumber(field, { read: oneToTen, current: 1, show: String, save });
      expect(save).not.toHaveBeenCalled();
      expect(field.value).toBe('1');
    }
  });

  it('shows the saved value when what was typed is refused', () => {
    const field = typedField('');
    const save = vi.fn();
    commitNumber(field, { read: oneToTen, current: 7, show: String, save });
    expect(save).not.toHaveBeenCalled();
    expect(field.value).toBe('7');
  });
});
