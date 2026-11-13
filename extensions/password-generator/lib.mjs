// Password generation, ported from Raycast's Password Generator. Pure apart
// from the word list, which is read once on first use.
import crypto from 'node:crypto';
import {readFileSync} from 'node:fs';

// Digits and letters that are easy to confuse (0/O, 1/l/I) are left out.
export const NUMBERS = '23456789';
export const SYMBOLS = '!@#$*^&%';
export const LETTERS = 'abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ';
export const MIN_LENGTH = 5;
export const MAX_LENGTH = 64;

let words = null;
/** The 370k-word list the memorable format draws from. */
export function wordList() {
  words ??= readFileSync(new URL('./words.txt', import.meta.url), 'utf8').split('\n').map(w => w.trim()).filter(Boolean);
  return words;
}

/** A password of `length` characters with at least one number and one symbol when asked for. */
export function generatePassword(length, useNumbers, useChars) {
  let charset = LETTERS;
  if (useNumbers) charset += NUMBERS;
  if (useChars) charset += SYMBOLS;
  const result = [];
  // Reserve guaranteed positions, then shuffle so they are not predictable.
  if (useNumbers) result.push(NUMBERS[crypto.randomInt(NUMBERS.length)]);
  if (useChars) result.push(SYMBOLS[crypto.randomInt(SYMBOLS.length)]);
  while (result.length < length) result.push(charset[crypto.randomInt(charset.length)]);
  for (let i = result.length - 1; i > 0; i--) {
    const j = crypto.randomInt(i + 1);
    [result[i], result[j]] = [result[j], result[i]];
  }
  return result.join('');
}

/**
 * Fills a format such as `{word:capitalize}-{number:4}{symbol}`:
 * {word[:uppercase|lowercase|capitalize]}, {symbol:n}, {number:n}, {random:n}.
 * Unknown placeholders stay as typed; no word repeats within one password.
 */
export function generateCustomPassword(format, list = wordList()) {
  const used = new Set();
  const chars = (charset, n) => Array.from({length: n}, () => charset[crypto.randomInt(charset.length)]).join('');
  return format.replace(/{(\w+)(:(\w+))?}/g, (match, type, _, modifier) => {
    const n = parseInt(modifier, 10) || 1;
    switch (type) {
      case 'word': {
        let word;
        do { word = list[crypto.randomInt(list.length)].trim(); } while (used.has(word) && used.size < list.length);
        used.add(word);
        if (modifier === 'uppercase') return word.toUpperCase();
        if (modifier === 'lowercase') return word.toLowerCase();
        if (modifier === 'capitalize') return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
        return crypto.randomInt(2) === 0 ? word : word.toUpperCase();
      }
      case 'random': return generatePassword(n, true, true);
      case 'symbol': return chars(SYMBOLS, n);
      case 'number': return chars(NUMBERS, n);
      default: return match;
    }
  });
}

/** The length typed by the user, or the reason it cannot be used. */
export function parseLength(text) {
  const length = parseInt(String(text ?? ''), 10);
  if (!Number.isFinite(length)) return {error: 'Password length must be a number'};
  if (length < MIN_LENGTH || length > MAX_LENGTH) return {error: `Password length must be between ${MIN_LENGTH} and ${MAX_LENGTH}`};
  return {length};
}

export const FORMAT_HELP = `{word} (random uppercase or lowercase word)
{word:uppercase} (random uppercase word)
{word:lowercase} (random lowercase word)
{word:capitalize} (random word, first letter capitalized)
{symbol:n} (n number of random symbols)
{number:n} (n number of random digits)
{random:n} (n random characters, digits, symbols)`;
