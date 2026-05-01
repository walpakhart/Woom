import { describe, expect, it } from 'vitest';
import { parseSlashCommand } from './slashCommands';

describe('parseSlashCommand', () => {
  it('matches known commands by themselves', () => {
    expect(parseSlashCommand('/compact')).toBe('compact');
    expect(parseSlashCommand('/clear')).toBe('clear');
    expect(parseSlashCommand('/usage')).toBe('usage');
    expect(parseSlashCommand('/help')).toBe('help');
  });

  it('is case-insensitive on the word', () => {
    expect(parseSlashCommand('/Compact')).toBe('compact');
    expect(parseSlashCommand('/HELP')).toBe('help');
  });

  it('trims surrounding whitespace', () => {
    expect(parseSlashCommand('  /compact   ')).toBe('compact');
  });

  it('does not match unknown commands', () => {
    expect(parseSlashCommand('/foo')).toBeNull();
    expect(parseSlashCommand('/checkout')).toBeNull(); /* future, not yet wired */
  });

  it('does not match when there is text after the command', () => {
    /* "/compact please" is a normal message starting with slash —
     * we never want to silently swallow it. */
    expect(parseSlashCommand('/compact please')).toBeNull();
    expect(parseSlashCommand('/clear and reset')).toBeNull();
  });

  it('does not match plain text', () => {
    expect(parseSlashCommand('compact')).toBeNull();
    expect(parseSlashCommand('hello world')).toBeNull();
    expect(parseSlashCommand('')).toBeNull();
  });

  it('does not match a single slash', () => {
    expect(parseSlashCommand('/')).toBeNull();
  });

  it('does not match commands with non-letter characters', () => {
    expect(parseSlashCommand('/compact1')).toBeNull();
    expect(parseSlashCommand('/com-pact')).toBeNull();
  });
});
