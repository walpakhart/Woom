import { describe, expect, it } from 'vitest';
import { parseSlashCommand, parseSlashCommandWithArgs } from './slashCommands';

describe('parseSlashCommand', () => {
  it('matches known commands by themselves', () => {
    expect(parseSlashCommand('/compact')).toBe('compact');
    expect(parseSlashCommand('/clear')).toBe('clear');
    expect(parseSlashCommand('/usage')).toBe('usage');
    expect(parseSlashCommand('/help')).toBe('help');
    expect(parseSlashCommand('/ps')).toBe('ps');
    expect(parseSlashCommand('/preview')).toBe('preview');
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

  it('does not match when there is text after an arg-less command', () => {
    /* "/compact please" is a normal message starting with slash —
     * we never want to silently swallow it. */
    expect(parseSlashCommand('/compact please')).toBeNull();
    expect(parseSlashCommand('/clear and reset')).toBeNull();
  });

  it('rejects /kill bare (needs an arg)', () => {
    /* `/kill` is arg-bearing — strict-exact must reject it; the
     *  parseSlashCommandWithArgs path is responsible. */
    expect(parseSlashCommand('/kill')).toBeNull();
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

describe('parseSlashCommandWithArgs', () => {
  it('matches arg-bearing commands with args', () => {
    expect(parseSlashCommandWithArgs('/preview pnpm dev'))
      .toEqual({ name: 'preview', args: 'pnpm dev' });
    expect(parseSlashCommandWithArgs('/kill 5173'))
      .toEqual({ name: 'kill', args: '5173' });
  });

  it('trims surrounding whitespace and arg whitespace', () => {
    expect(parseSlashCommandWithArgs('   /preview   pnpm dev   '))
      .toEqual({ name: 'preview', args: 'pnpm dev' });
  });

  it('returns null without args', () => {
    expect(parseSlashCommandWithArgs('/preview')).toBeNull();
    expect(parseSlashCommandWithArgs('/kill')).toBeNull();
  });

  it('returns null for arg-less commands even with args', () => {
    /* `/compact please` is not a valid command — it's a regular
     *  message. arg-less commands stay strict-exact. */
    expect(parseSlashCommandWithArgs('/compact please')).toBeNull();
    expect(parseSlashCommandWithArgs('/help me')).toBeNull();
  });

  it('returns null for unknown commands', () => {
    expect(parseSlashCommandWithArgs('/foo bar')).toBeNull();
  });
});
