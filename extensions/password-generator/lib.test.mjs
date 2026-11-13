import test from 'node:test';
import assert from 'node:assert/strict';
import {LETTERS, NUMBERS, SYMBOLS, generateCustomPassword, generatePassword, parseLength, wordList} from './lib.mjs';

const has = (password, set) => [...set].some(c => password.includes(c));

test('generates passwords of the requested length from the right characters', () => {
  assert.equal(generatePassword(10, false, false).length, 10);
  for (let i = 0; i < 50; i++) {
    const letters = generatePassword(20, false, false);
    assert.ok(!/[2-9!@#$*^&%]/.test(letters));
    assert.ok(has(generatePassword(10, true, false), NUMBERS));
    assert.ok(has(generatePassword(10, false, true), SYMBOLS));
    const both = generatePassword(10, true, true);
    assert.ok(has(both, NUMBERS) && has(both, SYMBOLS));
  }
  const tiny = generatePassword(2, true, true);
  assert.ok(tiny.length === 2 && has(tiny, NUMBERS) && has(tiny, SYMBOLS));
  for (const c of generatePassword(50, true, true)) assert.ok((LETTERS + NUMBERS + SYMBOLS).includes(c));
  assert.ok(new Set(Array.from({length: 10}, () => generatePassword(10, true, true)[0])).size > 1);
});

test('fills memorable formats', () => {
  const list = ['apple', 'Banana', 'cherry'];
  assert.match(generateCustomPassword('{word:uppercase}-{number:4}{symbol:2}', list), /^(APPLE|BANANA|CHERRY)-[2-9]{4}[!@#$*^&%]{2}$/);
  assert.match(generateCustomPassword('{word:capitalize}', list), /^(Apple|Banana|Cherry)$/);
  assert.equal(generateCustomPassword('{random:12}', list).length, 12);
  assert.equal(generateCustomPassword('{unknown} x', list), '{unknown} x');
  const words = generateCustomPassword('{word:lowercase} {word:lowercase} {word:lowercase}', list).split(' ');
  assert.equal(new Set(words).size, 3);
  assert.ok(wordList().length > 300000);
});

test('validates the length like Raycast', () => {
  assert.deepEqual(parseLength('16'), {length: 16});
  assert.match(parseLength('abc').error, /must be a number/);
  assert.match(parseLength('4').error, /between 5 and 64/);
  assert.match(parseLength('65').error, /between 5 and 64/);
});
