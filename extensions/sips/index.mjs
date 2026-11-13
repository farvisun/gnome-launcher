import {createHash} from 'node:crypto';
import {existsSync, mkdirSync, readdirSync, statSync} from 'node:fs';
import path from 'node:path';
import {host, push, serve, update} from '../../sdk/index.mjs';
import {activeFilters, FILTERS} from './filters.mjs';
import {CACHE, deliver, selectedImages} from './io.mjs';
import {convertFormats, evaluate, parseColor, pluralize} from './lib.mjs';
import {canWritePdf, INSTALL_HINT, magick, magickPath, MissingTool, which, writable} from './magick.mjs';
import * as ops from './ops.mjs';
import {colorPair, PATTERNS, sizeGroups, sizeThumbnail, VARIANTS} from './patterns.mjs';
import {convertList, filterGrid, missingMagick, patternGrid, sizeGrid} from './views.mjs';

const fail = (title, message) => host.toast({style: 'failure', title, message});

/**
 * Raycast's runOperation: an animated toast while it works, then a HUD (no-view
 * commands, which close Lumen) or a success toast; a failure keeps Lumen open.
 */
async function runOperation(params, {inProgress, success, failure, operation, noView = true}) {
  if (!magickPath()) return void await fail('ImageMagick is not installed', INSTALL_HINT);
  const input = await selectedImages(params);
  if (!input.files.length) {
    return void await fail('No images selected', 'Select images in Files (or copy them, with Clipboard input), or choose them when asked.');
  }
  await host.toast({style: 'animated', title: inProgress});
  // Long batches keep the request alive; an empty update changes nothing on screen.
  const heartbeat = setInterval(() => update(null), 4000);
  const context = {preferences: params.preferences, signal: params.signal, fromClipboard: input.fromClipboard, folder: input.folder};
  const count = pluralize(input.files.length);
  try {
    const results = await operation(input.files, context);
    await deliver(results, context);
    if (noView) await host.hud(`${success} ${count}`);
    else await host.toast({style: 'success', title: `${success} ${count}`, message: results.length === 1 ? path.basename(results[0]) : undefined});
  } catch (error) {
    if (params.signal.aborted) throw error;
    await fail(`${failure} ${count}`, error instanceof MissingTool ? INSTALL_HINT : error.message);
  } finally {
    clearInterval(heartbeat);
  }
  return null;
}

// ---- Cached images: filter thumbnails, size outlines, pattern previews ----

const hash = value => createHash('sha1').update(JSON.stringify(value)).digest('hex').slice(0, 12);
const SAMPLES = ['/usr/share/backgrounds/mendhak-Red_Acer.jpg', '/usr/share/backgrounds/mendhak-Bluebells_Suspended_In_Time.jpg'];

/** A photo to show filters on: a system wallpaper, or a generated one. */
async function sample(signal) {
  const target = path.join(CACHE, 'sample.png');
  if (existsSync(target)) return target;
  mkdirSync(CACHE, {recursive: true});
  const photos = SAMPLES.filter(existsSync);
  const backgrounds = '/usr/share/backgrounds';
  if (!photos.length && existsSync(backgrounds)) {
    photos.push(...readdirSync(backgrounds).filter(n => /\.(jpe?g|png|webp)$/i.test(n)).map(n => path.join(backgrounds, n)));
  }
  const source = photos[0] ? [`${photos[0]}[0]`] : ['-size', '192x192', 'plasma:fractal'];
  await magick([...source, '-resize', '192x192^', '-gravity', 'center', '-extent', '192x192', target], {signal});
  return target;
}

/** Makes the images that are missing, a few at a time, telling `progress` as they land. */
async function generate(jobs, signal, progress) {
  const pending = jobs.filter(job => !existsSync(job.target));
  let last = Date.now();
  for (let i = 0; i < pending.length; i += 4) {
    await Promise.all(pending.slice(i, i + 4).map(async job => {
      mkdirSync(path.dirname(job.target), {recursive: true});
      try { await job.make(job.target); } catch (error) { if (signal.aborted) throw error; console.error(error.message); }
    }));
    if (progress && Date.now() - last > 400) { progress(); last = Date.now(); }
  }
}
const existing = jobs => Object.fromEntries(jobs.filter(j => existsSync(j.target)).map(j => [j.id, j.target]));

function filterJobs(filters, base, signal) {
  const dir = path.join(CACHE, `thumbnails-${hash(FILTERS.map(f => [f.id, f.args({w: 192, h: 192})]))}`);
  return filters.map(f => ({id: f.id, target: path.join(dir, `${f.id}.png`), make: target => ops.filterPreview(base, f, target, signal)}));
}

async function filtersView({query, preferences, signal}) {
  if (!magickPath()) return missingMagick('Search filters...');
  const filters = activeFilters(preferences.hiddenFilters);
  const base = await sample(signal);
  const jobs = filterJobs(filters, base, signal);
  const render = () => filterGrid(filters, {query, thumbnails: existing(jobs)});
  update(render());
  await generate(jobs, signal, () => update(render()));
  return render();
}

/** A live preview of the selected filter on the first selected image, as Raycast shows. */
async function filterSelected(params) {
  const {item, query, preferences, signal} = params;
  if (!preferences.showPreviews || !magickPath()) return null;
  const filter = FILTERS.find(f => f.id === item);
  const {files} = await selectedImages(params, {interactive: false});
  const file = files[0];
  if (!filter || !file || statSync(file).size >= 800_000) return null;
  const filters = activeFilters(preferences.hiddenFilters);
  const image = path.join(CACHE, 'previews', `${hash([file, statSync(file).mtimeMs])}-${filter.id}.png`);
  if (!existsSync(image)) await ops.filterPreview(file, filter, image, signal);
  return filterGrid(filters, {query, thumbnails: existing(filterJobs(filters, path.join(CACHE, 'sample.png'), signal)), preview: {id: filter.id, image}});
}

function sizeJobs(signal) {
  return sizeGroups().flatMap(group => group.sizes.map(s => ({
    id: `${s.width}x${s.height}`,
    target: path.join(CACHE, 'sizes', `${s.width}x${s.height}.png`),
    make: target => magick([...sizeThumbnail(s, group.color), target], {signal}),
  })));
}

async function sizesView({query, signal}) {
  if (!magickPath()) return missingMagick('Search image sizes...');
  const jobs = sizeJobs(signal);
  const render = () => sizeGrid(sizeGroups(), {query, thumbnails: existing(jobs)});
  if (jobs.some(j => !existsSync(j.target))) update(render());
  await generate(jobs, signal, () => update(render()));
  return render();
}

/** Each pattern in a few colour variants; `colors` keeps earlier picks. */
function variants(saved = []) {
  const kept = new Map(saved.map(v => [v.id, v.colors]));
  return PATTERNS.flatMap(pattern => Array.from({length: pattern.id === 'random' ? 1 : VARIANTS}, (_, i) => {
    const id = `${pattern.id}-${i + 1}`;
    return {id, name: pattern.name, title: pattern.id === 'random' ? pattern.name : `${pattern.name} ${i + 1}`, colors: kept.get(id) ?? colorPair()};
  }));
}
const patternOf = variant => PATTERNS.find(p => variant.id.startsWith(`${p.id}-`));
function previewSize(width, height) {
  const scale = 128 / Math.max(width, height);
  return [Math.max(4, Math.round(width * scale)), Math.max(4, Math.round(height * scale))];
}
function previewJobs(list, width, height, signal) {
  const [w, h] = previewSize(width, height);
  return list.map(v => ({
    id: v.id,
    target: path.join(CACHE, 'patterns', `${hash([v.id, v.colors, w, h])}.png`),
    make: target => magick([...patternOf(v).args(w, h, v.colors), target], {signal}),
  }));
}

async function patternsView({width, height, saved, query, preferences, signal}) {
  const list = variants(saved);
  const jobs = previewJobs(list, width, height, signal);
  const render = () => patternGrid({width, height, variants: list, query, previews: preferences.generatePreviews === false ? {} : existing(jobs)});
  if (preferences.generatePreviews !== false) {
    update(render());
    await generate(jobs, signal, () => update(render()));
  }
  return render();
}

const placeholderArgs = (width, height) => ['-size', `${width}x${height}`, 'xc:#C0C0C0'];

async function createView(params) {
  const {context} = params;
  return context?.width
    ? patternsView({...params, width: context.width, height: context.height, saved: context.variants})
    : sizesView(params);
}

async function createAction(params) {
  const {item, action, context, preferences, signal} = params;
  const where = {preferences, signal, folder: (await selectedImages(params, {interactive: false})).folder};
  if (!context?.width) {
    const [width, height] = item.split('x').map(Number);
    if (action === 'primary') return push(await patternsView({...params, width, height, saved: [], query: ''}));
    const scratch = path.join(CACHE, 'results', `Placeholder ${width}x${height}.png`);
    if (action === 'placeholder') {
      const [file] = await ops.create(`Placeholder ${width}x${height}`, placeholderArgs(width, height), where);
      await deliver([file], where);
      return void await host.toast({style: 'success', title: 'Created placeholder', message: file});
    }
    mkdirSync(path.dirname(scratch), {recursive: true});
    await magick([...placeholderArgs(width, height), scratch], {signal});
    if (action === 'copy-image') await host.copyFile(scratch);
    else await host.copy(ops.dataUrl(scratch));
    return void await host.toast({style: 'success', title: action === 'copy-image' ? 'Copied placeholder image' : 'Copied data URL'});
  }
  const {width, height} = context;
  const list = variants(context.variants);
  const variant = list.find(v => v.id === item);
  if (!variant) return null;
  if (action === 'randomize') {
    for (const v of list) v.colors = colorPair();
    return patternsView({...params, width, height, saved: list, query: params.query});
  }
  const pattern = patternOf(variant);
  if (action === 'primary') {
    await host.toast({style: 'animated', title: `Creating ${variant.name}…`});
    const [file] = await ops.create(`${variant.name} ${width}x${height}`, pattern.args(width, height, variant.colors), where);
    await deliver([file], where);
    return void await host.toast({style: 'success', title: `Created ${variant.name}`, message: file});
  }
  const [preview] = previewJobs([variant], width, height, signal);
  if (!existsSync(preview.target)) await generate([preview], signal);
  if (action === 'copy-image') await host.copyFile(preview.target);
  else await host.copy(ops.dataUrl(preview.target));
  return void await host.toast({style: 'success', title: action === 'copy-image' ? 'Copied preview image' : 'Copied data URL'});
}

async function formats(preferences) {
  const all = await writable();
  if (!(await canWritePdf())) all.delete('PDF');
  return convertFormats(all, preferences, {potrace: Boolean(which('potrace'))});
}

const noArguments = ({arguments: args}) => args ?? {};

serve({
  'flip-horizontally': {run: params => runOperation(params, {
    inProgress: 'Flipping in progress...', success: 'Flipped', failure: 'Failed to flip',
    operation: (files, context) => ops.flip(files, context, 'horizontal')})},
  'flip-vertically': {run: params => runOperation(params, {
    inProgress: 'Flipping in progress...', success: 'Flipped', failure: 'Failed to flip',
    operation: (files, context) => ops.flip(files, context, 'vertical')})},
  rotate: {run: async params => {
    let angle;
    try { angle = evaluate(noArguments(params).angle); } catch { return void await fail('Angle must be a number'); }
    if (params.preferences.rotationUnit === 'radians') angle *= 180 / Math.PI;
    return runOperation(params, {inProgress: 'Rotation in progress...', success: 'Rotated', failure: 'Failed to rotate',
      operation: (files, context) => ops.rotate(files, context, angle)});
  }},
  resize: {run: async params => {
    const {width: w = '', height: h = ''} = noArguments(params);
    if (!w.trim() && !h.trim()) return void await fail('Must specify either width or height');
    const width = w.trim() ? Number(w) : null;
    const height = h.trim() ? Number(h) : null;
    if (width !== null && !(Number.isInteger(width) && width > 0)) return void await fail('Width must be an integer');
    if (height !== null && !(Number.isInteger(height) && height > 0)) return void await fail('Height must be an integer');
    return runOperation(params, {inProgress: 'Resizing in progress...', success: 'Resized', failure: 'Failed to resize',
      operation: (files, context) => ops.resize(files, context, width, height)});
  }},
  scale: {run: async params => {
    let factor;
    try { factor = evaluate(noArguments(params).scaleFactor); } catch { factor = NaN; }
    if (!(factor > 0)) return void await fail('Scale factor must be a number');
    return runOperation(params, {inProgress: 'Scaling in progress...', success: 'Scaled', failure: 'Failed to scale',
      operation: (files, context) => ops.scale(files, context, factor)});
  }},
  pad: {run: async params => {
    const {amount = '', hexcolor = ''} = noArguments(params);
    const padding = Number(amount.trim());
    if (!(Number.isInteger(padding) && padding > 0)) return void await fail('Padding amount must be a positive integer');
    const color = parseColor(hexcolor.trim() || params.preferences.defaultPadColor || '#FFFFFF');
    if (!color) return void await fail('Invalid HEX Color');
    return runOperation(params, {inProgress: 'Padding in progress...', success: 'Padded', failure: 'Failed to pad',
      operation: (files, context) => ops.pad(files, context, padding, color)});
  }},
  optimize: {run: async params => {
    const text = (noArguments(params).optimizationFactor ?? '').trim();
    const amount = text ? Number(text) : 50;
    if (!Number.isFinite(amount) || amount <= 0 || amount > 100) return void await fail('Invalid optimization factor');
    return runOperation(params, {inProgress: 'Optimization in progress...', success: 'Optimized', failure: 'Failed to optimize',
      operation: (files, context) => ops.optimize(files, context, amount)});
  }},
  'strip-exif': {run: params => runOperation(params, {
    inProgress: 'Sanitizing...', success: 'Sanitized', failure: 'Failed to sanitize',
    operation: (files, context) => ops.stripExif(files, context)})},
  'remove-bg': {run: async params => {
    if (!which('rembg')) {
      return void await fail('Remove Background needs rembg', 'Install it with: pipx install "rembg[cli]", then run rembg once in a terminal to download its model.');
    }
    const {bgcolor = '', crop = ''} = noArguments(params);
    const text = bgcolor.trim() || String(params.preferences.defaultBgColor ?? '').trim();
    const color = text ? parseColor(text) : null;
    if (text && !color) {
      return void await fail('Invalid color string provided.', `'${text}' is not a valid color string. Please provide a valid hex color (e.g. #FF0000FF) or named HTML color (e.g. Red).`);
    }
    const cropToSubject = crop === 'yes' || (crop !== 'no' && params.preferences.cropByDefault !== false);
    const many = (params.launch_context?.files?.length ?? 0) > 1;
    return runOperation(params, {
      inProgress: `Removing ${many ? 'backgrounds' : 'background'}...`, success: 'Removed background from', failure: 'Failed to remove background from',
      operation: (files, context) => ops.removeBackground(files, context, {color, crop: cropToSubject})});
  }},
  convert: {
    run: async ({query, preferences}) => (magickPath() ? convertListFor(preferences, query) : missingMagick('Search image transformations...')),
    action: async params => {
      if (params.action === 'preferences') return void await host.openPreferences();
      const format = (await formats(params.preferences)).find(f => f.name === params.item);
      if (!format) return null;
      return runOperation(params, {inProgress: 'Conversion in progress...', success: 'Converted', failure: 'Failed to convert', noView: false,
        operation: (files, context) => ops.convert(files, context, format)});
    },
  },
  'apply-filter': {
    run: filtersView,
    select: filterSelected,
    action: async params => {
      const filter = FILTERS.find(f => f.id === params.item);
      if (!filter) return null;
      return runOperation(params, {inProgress: 'Filtering in progress...', success: 'Applied filter to', failure: 'Failed to apply filter to', noView: false,
        operation: (files, context) => ops.applyFilter(files, context, filter)});
    },
  },
  'create-image': {run: createView, action: createAction},
});

async function convertListFor(preferences, query) {
  return convertList(await formats(preferences), query);
}
