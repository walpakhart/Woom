import { describe, it, expect } from 'vitest';
import { foldStatus } from './gitDecorations';

describe('foldStatus', () => {
  it('picks modified over untracked', () => {
    expect(foldStatus(['?', 'M'])).toBe('M');
  });
  it('picks deleted over added', () => {
    expect(foldStatus(['A', 'D'])).toBe('D');
  });
  it('keeps a lone untracked', () => {
    expect(foldStatus(['?'])).toBe('?');
  });
  it('returns empty for no codes', () => {
    expect(foldStatus([])).toBe('');
  });
  it('conflict beats everything', () => {
    expect(foldStatus(['M', 'U', 'D'])).toBe('U');
  });
  it('ignores unknown codes when a known one is present', () => {
    expect(foldStatus(['X', 'A'])).toBe('A');
  });
});
