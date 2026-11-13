// The image operations, each a port of a Raycast sips operation onto
// ImageMagick. They return the paths they wrote.
import {copyFileSync, existsSync, readFileSync, rmSync} from 'node:fs';
import path from 'node:path';
import {magick, run, size, which} from './magick.mjs';
import {plan, writeAtomically} from './io.mjs';

const ext = file => path.extname(file).slice(1).toLowerCase();
const opaque = file => ['jpg', 'jpeg', 'jpe', 'bmp', 'pbm', 'ppm', 'pgm'].includes(ext(file));

/** Applies `args(source)` to each image, writing where the preferences say. */
async function each(files, context, args, options = {}) {
  const targets = plan(files, {...context, ...options});
  for (const [index, source] of files.entries()) {
    await writeAtomically(targets[index], async temporary => {
      await magick([source, ...(await args(source, targets[index])), temporary], {signal: context.signal});
    });
  }
  return targets;
}

const lossless = (context, file) => (context.preferences.useLosslessConversion
  ? ext(file) === 'webp' ? ['-define', 'webp:lossless=true'] : ['avif', 'heic'].includes(ext(file)) ? ['-quality', '100'] : []
  : []);

export const flip = (files, context, direction) =>
  each(files, context, (_s, target) => [direction === 'horizontal' ? '-flop' : '-flip', ...lossless(context, target)]);

/** Clockwise, like sips; corners outside the turned image are clear (white for JPEG). */
export const rotate = (files, context, degrees) =>
  each(files, context, (_s, target) => ['-background', opaque(target) ? 'white' : 'none', '-rotate', String(degrees), ...lossless(context, target)]);

export const resize = (files, context, width, height) =>
  each(files, context, (_s, target) => ['-resize', width && height ? `${width}x${height}!` : width ? `${width}` : `x${height}`, ...lossless(context, target)]);

export const scale = (files, context, factor) =>
  each(files, context, (_s, target) => ['-resize', `${factor * 100}%`, ...lossless(context, target)]);

/** Adds `amount` pixels to each dimension, split around the image, as sips --padToHeightWidth does. */
export const pad = (files, context, amount, color) =>
  each(files, context, async (source, target) => {
    const {width, height} = await size(source, context.signal);
    return ['-gravity', 'center', '-background', color, '-extent', `${width + amount}x${height + amount}`, ...lossless(context, target)];
  });

export async function applyFilter(files, context, filter) {
  return each(files, context, async source => {
    const {width, height} = await size(source, context.signal);
    return filter.args({w: width, h: height});
  });
}

/** A filter on a small copy of `source`, for thumbnails and live previews. */
export async function filterPreview(source, filter, target, signal, side = 192) {
  await writeAtomically(target, async temporary => {
    await magick([`${source}[0]`, '-auto-orient', '-resize', `${side}x${side}^`, '-gravity', 'center', '-extent', `${side}x${side}`,
      ...filter.args({w: side, h: side}), temporary], {signal});
  });
  return target;
}

/** Re-encodes smaller, into "<name>-optimized.<ext>", as Raycast does. `amount` is 0–100. */
export async function optimize(files, context, amount) {
  const svgo = which('svgo');
  const targets = plan(files, {...context, suffix: '-optimized'});
  for (const [index, source] of files.entries()) {
    const target = targets[index];
    await writeAtomically(target, async temporary => {
      const type = ext(source);
      if (type === 'svg') {
        if (!svgo) throw new Error('Optimizing SVG images needs svgo: npm install -g svgo');
        await run(svgo, ['-i', source, '-o', temporary], {signal: context.signal});
        return;
      }
      const quality = String(Math.max(1, Math.min(100, Math.round(100 - amount))));
      const args = {
        jpg: ['-strip', '-quality', quality], jpeg: ['-strip', '-quality', quality],
        webp: ['-strip', '-quality', quality], avif: ['-strip', '-quality', quality],
        png: ['-strip', '-define', 'png:compression-level=9', '-define', 'png:compression-filter=5', '-define', 'png:compression-strategy=1'],
        gif: ['-layers', 'Optimize'],
      }[type] ?? ['-strip'];
      await magick([source, ...args, temporary], {signal: context.signal});
    });
  }
  return targets;
}

/** Removes metadata without re-encoding when ExifTool is installed. */
export async function stripExif(files, context) {
  const exiftool = which('exiftool');
  const targets = plan(files, context);
  for (const [index, source] of files.entries()) {
    await writeAtomically(targets[index], async temporary => {
      if (exiftool) {
        copyFileSync(source, temporary);
        await run(exiftool, ['-all=', '-overwrite_original', '-q', temporary], {signal: context.signal});
      } else {
        await magick([source, '-strip', temporary], {signal: context.signal});
      }
    });
  }
  return targets;
}

/** Converts to `format` ({coder, extension}); the originals stay. SVG is traced with potrace. */
export async function convert(files, context, format) {
  const targets = plan(files, {...context, extension: format.extension});
  for (const [index, source] of files.entries()) {
    await writeAtomically(targets[index], async temporary => {
      if (format.name === 'SVG') {
        const bitmap = await magick([`${source}[0]`, '-background', 'white', '-alpha', 'remove', '-colorspace', 'Gray', '-threshold', '50%', 'bmp:-'], {signal: context.signal});
        await run(which('potrace'), ['-s', '--tight', '-o', temporary, '-'], {signal: context.signal, input: bitmap});
        return;
      }
      const args = [];
      if (opaque(temporary)) args.push('-background', 'white', '-alpha', 'remove', '-alpha', 'off');
      if (format.name === 'ICO') args.push('-resize', '256x256>');
      if (['WEBP', 'AVIF'].includes(format.name)) args.push(...lossless(context, temporary));
      await magick([source, ...args, `${format.coder}:${temporary}`], {signal: context.signal});
    });
  }
  return targets;
}

/** Cuts the subject out with rembg, then optionally fills a colour and crops. */
export async function removeBackground(files, context, {color, crop}) {
  const rembg = which('rembg');
  if (!rembg) throw new Error('Remove Background needs rembg: pipx install "rembg[cli]", then run it once in a terminal to download its model');
  const keep = context.preferences.preserveFormat !== false;
  const targets = files.map(source => plan([source], {...context, extension: keep ? ext(source) : 'png'})[0]);
  for (const [index, source] of files.entries()) {
    const cutout = path.join(path.dirname(targets[index]), `.${path.basename(source)}.lumen-cutout.png`);
    try {
      await run(rembg, ['i', source, cutout], {signal: context.signal});
      await writeAtomically(targets[index], async temporary => {
        const args = [cutout];
        if (crop) args.push('-trim', '+repage');
        if (color) args.push('-background', color, '-alpha', 'remove', '-alpha', 'off');
        else if (opaque(temporary)) args.push('-background', 'white', '-alpha', 'remove', '-alpha', 'off');
        await magick([...args, temporary], {signal: context.signal});
      });
    } finally {
      rmSync(cutout, {force: true});
    }
  }
  return targets;
}

/** Renders `args` (a generator) into a new PNG named `name`, saved as generated images are. */
export async function create(name, args, context) {
  const [target] = plan([path.join(context.folder ?? '/', `${name}.png`)], {...context, generated: true});
  await writeAtomically(target, temporary => magick([...args, temporary], {signal: context.signal}));
  return [target];
}

/** A PNG as a data URL, for "Copy … Data URL". */
export const dataUrl = file => `data:image/png;base64,${readFileSync(file).toString('base64')}`;
export {existsSync};
