import test from 'node:test';
import assert from 'node:assert/strict';
import {activeFilters, FILTERS} from './filters.mjs';
import {convertFormats, destinations, evaluate, isImage, parseColor, pluralize, writableFormats} from './lib.mjs';
import {colorPair, PATTERNS, sizeGroups} from './patterns.mjs';

test('reads arithmetic angles and factors like mathjs', () => {
  assert.equal(evaluate('90'), 90);
  assert.equal(evaluate('90*2'), 180);
  assert.equal(evaluate(' -45 + 15 '), -30);
  assert.equal(evaluate('2^3'), 8);
  assert.equal(evaluate('(1+1)*1.5'), 3);
  assert.ok(Math.abs(evaluate('pi/2') - Math.PI / 2) < 1e-12);
  for (const bad of ['', 'abc', '90deg', '1/0', '2*']) assert.throws(() => evaluate(bad), /Cannot read/);
});

test('accepts hex and named colours', () => {
  assert.equal(parseColor('fff'), '#FFF');
  assert.equal(parseColor('#ff0000aa'), '#FF0000AA');
  assert.equal(parseColor('Red'), 'red');
  assert.equal(parseColor('#12345'), null);
  assert.equal(parseColor(''), null);
  assert.equal(pluralize(1), '1 image');
  assert.equal(pluralize(3), '3 images');
  assert.ok(isImage('/a/b.JPG') && !isImage('/a/b.txt'));
});

test('places results as Raycast does', () => {
  const dirs = {downloads: '/home/u/Downloads', desktop: '/home/u/Desktop', scratch: '/cache/results', folder: '/home/u/Pictures'};
  const taken = new Set(['/p/a-2.png', '/home/u/Desktop/a.png']);
  const exists = p => taken.has(p) || p === '/p/a.png';
  const at = (handling, extra = {}) => destinations(['/p/a.png'], {handling, ...dirs, ...extra}, exists)[0];
  assert.equal(at('replaceOriginal'), '/p/a.png');
  assert.equal(at('saveInContainingFolder'), '/p/a-3.png');
  assert.equal(at('saveToDesktop'), '/home/u/Desktop/a-2.png');
  assert.equal(at('saveToDownloads'), '/home/u/Downloads/a.png');
  assert.equal(at('copyToClipboard'), '/cache/results/a.png');
  assert.equal(at('replaceOriginal', {suffix: '-optimized'}), '/p/a-optimized.png');
  assert.equal(at('replaceOriginal', {extension: 'webp'}), '/p/a.webp');
  assert.equal(at('replaceOriginal', {generated: true}), '/home/u/Pictures/a.png');
  // Two sources with one name never share a target.
  assert.deepEqual(destinations(['/x/a.png', '/y/a.png'], {handling: 'saveToDownloads', ...dirs}, () => false),
    ['/home/u/Downloads/a.png', '/home/u/Downloads/a-2.png']);
});

test('offers the formats this ImageMagick writes', () => {
  const listing = `   Format  Mode  Description
-------------------------------------------------------------------------------
     AVIF  HEIC      rw+   AV1 Image File Format (1.21.2)
     HEIC  HEIC      r--   High Efficiency Image Format (1.21.2)
     JPEG* JPEG      rw-   Joint Photographic Experts Group JFIF format
      PNG* PNG       rw-   Portable Network Graphics
     WEBP* WEBP      rw+   WebP Image Format`;
  const writable = writableFormats(listing);
  assert.deepEqual([...writable].sort(), ['AVIF', 'JPEG', 'PNG', 'WEBP']);
  const names = list => list.map(f => f.name);
  assert.deepEqual(names(convertFormats(writable, {})), ['AVIF', 'JPEG', 'PNG', 'WEBP']);
  assert.deepEqual(names(convertFormats(writable, {showPNG: false}, {potrace: true})), ['AVIF', 'JPEG', 'WEBP', 'SVG']);
  assert.equal(convertFormats(writable, {jpegExtension: 'jpg'}).find(f => f.name === 'JPEG').extension, 'jpg');
});

test('lists filters, hides by name or category, and makes valid recipes', () => {
  assert.equal(FILTERS.length, 45);
  assert.equal(new Set(FILTERS.map(f => f.id)).size, FILTERS.length);
  const hidden = activeFilters('Bloom, distortion');
  assert.ok(!hidden.some(f => f.name === 'Bloom' || f.category === 'Distortion'));
  assert.equal(hidden.length, 45 - 1 - 5);
  for (const f of FILTERS) {
    const args = f.args({w: 640, h: 480});
    assert.ok(args.length && args.every(a => typeof a === 'string'), f.name);
    assert.equal(args.filter(a => a === '(').length, args.filter(a => a === ')').length, f.name);
  }
});

test('groups sizes and draws patterns as Raycast does', () => {
  const groups = sizeGroups();
  assert.deepEqual(groups.map(g => g.title), ['Square', 'Wide', 'Tall', 'Extreme']);
  assert.equal(groups[0].sizes.length, 8);
  assert.ok(groups[1].sizes.every(s => s.width > s.height));
  assert.equal(groups[1].sizes.length, groups[2].sizes.length);
  let n = 0;
  const [a, b] = colorPair(() => [0, 0, 0.5][n++]);
  assert.notEqual(a, b);
  for (const p of PATTERNS) assert.ok(p.args(100, 50, ['#000', '#fff']).every(x => typeof x === 'string'), p.name);
});
