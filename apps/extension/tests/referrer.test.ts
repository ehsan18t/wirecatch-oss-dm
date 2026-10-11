import { describe, expect, it } from 'vitest';
import { refererFor } from '../src/lib/referrer';

describe('refererFor (strict-origin-when-cross-origin)', () => {
  it('sends the full page address to the same origin, without fragment or credentials', () => {
    expect(
      refererFor('https://example.com/files/a.zip', 'https://example.com/account/files?page=2#top'),
    ).toBe('https://example.com/account/files?page=2');
    expect(refererFor('https://example.com/a.zip', 'https://user:secret@example.com/private')).toBe(
      'https://example.com/private',
    );
  });

  it('sends only the origin to another origin, including another subdomain or port', () => {
    expect(
      refererFor('https://cdn.example.com/a.zip', 'https://example.com/account?token=abc'),
    ).toBe('https://example.com/');
    expect(refererFor('https://example.com:8443/a.zip', 'https://example.com/account')).toBe(
      'https://example.com/',
    );
    expect(refererFor('https://example.com/a.zip', 'http://example.com/page')).toBe(
      'http://example.com/',
    );
  });

  it('sends nothing from https to http', () => {
    expect(refererFor('http://example.com/a.zip', 'https://example.com/account')).toBeNull();
    expect(refererFor('http://other.test/a.zip', 'https://example.com/account')).toBeNull();
  });

  it('sends nothing without a web page', () => {
    expect(refererFor('https://example.com/a.zip', null)).toBeNull();
    expect(refererFor('https://example.com/a.zip', undefined)).toBeNull();
    expect(refererFor('https://example.com/a.zip', 'about:blank')).toBeNull();
    expect(refererFor('https://example.com/a.zip', 'not a url')).toBeNull();
  });
});
