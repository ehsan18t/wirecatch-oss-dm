import { describe, expect, it } from 'vitest';
import { decodePunycode } from './punycode';

describe('decodePunycode', () => {
  it('decodes the samples of RFC 3492, section 7.1, and names hosts carry', () => {
    expect(decodePunycode('egbpdaj6bu4bxfgehfvwxn')).toBe('ليهمابتكلموشعربي؟');
    expect(decodePunycode('ihqwcrb4cv8a8dqg056pqjye')).toBe('他们为什么不说中文');
    expect(decodePunycode('3B-ww4c5e180e575a65lsy2b')).toBe('3年B組金八先生');
    expect(decodePunycode('bcher-kva')).toBe('bücher');
    expect(decodePunycode('ls8h')).toBe('💩');
    expect(decodePunycode('')).toBe('');
  });

  it('refuses what is not Punycode: an early end, a character that is no digit, a value too large', () => {
    expect(decodePunycode('zz')).toBeNull();
    expect(decodePunycode('z')).toBeNull();
    expect(decodePunycode('bcher-kv!')).toBeNull();
    expect(decodePunycode('bücher-kva')).toBeNull();
    expect(decodePunycode('99999999999999')).toBeNull();
  });

  it('decodes text that no host name could hold, which the normalizer refuses by encoding it again', () => {
    expect(decodePunycode('a')).toBe('\u0080');
    expect(decodePunycode('ab-')).toBe('ab');
  });
});
