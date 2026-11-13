import test from 'node:test';
import assert from 'node:assert/strict';
import {FILTERS} from './filters.mjs';
import {FORMATS} from './lib.mjs';
import {sizeGroups} from './patterns.mjs';
import {convertList, filterGrid, patternGrid, sizeGrid} from './views.mjs';

test('convert lists formats and explains an empty list', () => {
  const view = convertList(FORMATS.slice(0, 3), 'av');
  assert.deepEqual(view.items.map(i => i.id), ['AVIF']);
  assert.equal(view.items[0].actions[0].title, 'Convert to AVIF');
  assert.equal(convertList([]).empty.title, 'No Formats Enabled');
});

test('the filter grid shows thumbnails and the live preview', () => {
  const view = filterGrid(FILTERS, {query: 'blur', thumbnails: {'box-blur': '/t/box.png'}, preview: {id: 'gaussian-blur', image: '/p/g.png'}});
  assert.equal(view.type, 'grid');
  assert.equal(view.selection, true);
  assert.ok(view.items.every(i => i.section === 'Blur' || /blur/i.test(i.title)));
  assert.deepEqual(view.items.find(i => i.id === 'box-blur').content, {image: '/t/box.png'});
  assert.deepEqual(view.items.find(i => i.id === 'gaussian-blur').content, {image: '/p/g.png'});
  assert.equal(view.items.find(i => i.id === 'motion-blur').content.icon, 'applications-graphics-symbolic');
});

test('sizes push patterns that keep their colours in the context', () => {
  const sizes = sizeGrid(sizeGroups());
  assert.ok(sizes.items.length <= 100);
  assert.equal(sizes.items[0].actions[0].title, 'Select Size 1024x1024');
  const variants = [{id: 'stripes-1', name: 'Stripes', title: 'Stripes 1', colors: ['#000', '#fff']}];
  const view = patternGrid({width: 512, height: 128, variants});
  assert.equal(view.aspect, '3/2');
  assert.deepEqual(view.context, {width: 512, height: 128, variants: [{id: 'stripes-1', colors: ['#000', '#fff']}]});
  assert.equal(view.items[0].actions[0].title, 'Create Stripes');
});
