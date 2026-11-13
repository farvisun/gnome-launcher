// Views for Image Modification, as Raycast's convert list and its filter and
// create-image grids. Pure: thumbnails are paths the caller made.
import {grid, list} from '../../sdk/index.mjs';
import {INSTALL_HINT} from './magick.mjs';

const matches = (query, ...texts) => {
  const needle = String(query ?? '').trim().toLowerCase();
  return !needle || texts.some(t => String(t).toLowerCase().includes(needle));
};

export function missingMagick(placeholder) {
  return list([], {placeholder, empty: {title: 'ImageMagick is not installed', description: INSTALL_HINT, icon: 'dialog-warning-symbolic'}});
}

/** The formats to convert to. */
export function convertList(formats, query = '') {
  const items = formats.filter(f => matches(query, f.name, f.extension)).map(f => ({
    id: f.name,
    title: f.name,
    icon: 'image-x-generic-symbolic',
    accessories: [{text: `.${f.extension}`, color: 'dim'}],
    actions: [{id: 'primary', title: `Convert to ${f.name}`}, {id: 'preferences', title: 'Open Command Preferences'}],
  }));
  return list(items, {
    placeholder: 'Search image transformations...',
    empty: formats.length
      ? {title: 'No matching formats', icon: 'image-x-generic-symbolic'}
      : {title: 'No Formats Enabled', description: 'Enable formats in the command preferences (Ctrl+K → Settings)', icon: 'image-x-generic-symbolic'},
  });
}

/**
 * The filters, grouped by category. `thumbnails` maps filter IDs to images;
 * `preview` is {id, image} for the live preview of the selected filter.
 */
export function filterGrid(filters, {query = '', thumbnails = {}, preview = null} = {}) {
  const items = filters.filter(f => matches(query, f.name, f.category)).map(f => ({
    id: f.id,
    title: f.name,
    subtitle: f.category,
    section: f.category,
    tooltip: f.description,
    content: preview?.id === f.id ? {image: preview.image}
      : thumbnails[f.id] ? {image: thumbnails[f.id]} : {icon: 'applications-graphics-symbolic', tint: 'dim'},
    actions: [{id: 'primary', title: `Apply ${f.name} Filter`}],
  }));
  return grid(items, {
    placeholder: 'Search filters...', columns: 5, selection: true,
    empty: {title: 'No filters match', description: 'Hidden filters are set in the command preferences.', icon: 'applications-graphics-symbolic'},
  });
}

/** Raycast's size choices; `thumbnails` maps "WxH" to an outline image. */
export function sizeGrid(groups, {query = '', thumbnails = {}} = {}) {
  const items = groups.flatMap(group => group.sizes.map(({width, height}) => {
    const id = `${width}x${height}`;
    return {
      id, title: id, section: group.title,
      content: thumbnails[id] ? {image: thumbnails[id]} : {icon: 'insert-image-symbolic'},
      actions: [
        {id: 'primary', title: `Select Size ${id}`},
        {id: 'placeholder', title: `Create ${id} Placeholder`},
        {id: 'copy-image', title: 'Copy Placeholder Image'},
        {id: 'copy-data-url', title: 'Copy Placeholder Data URL'},
      ],
    };
  })).filter(item => matches(query, item.title, item.section));
  return grid(items, {placeholder: 'Search image sizes...', columns: 6});
}

/** The pattern generators for one size, a few colour variants each. */
export function patternGrid({width, height, variants, previews = {}, query = ''}) {
  const items = variants.filter(v => matches(query, v.name)).map(v => ({
    id: v.id,
    title: v.title,
    content: previews[v.id] ? {image: previews[v.id]} : {icon: 'insert-image-symbolic'},
    actions: [
      {id: 'primary', title: `Create ${v.name}`},
      {id: 'randomize', title: 'Randomize Colors'},
      {id: 'copy-image', title: 'Copy Preview Image'},
      {id: 'copy-data-url', title: 'Copy Preview Data URL'},
    ],
  }));
  return grid(items, {
    placeholder: `Search patterns for ${width}×${height}...`, columns: 5,
    aspect: width / height > 1.4 ? '3/2' : height / width > 1.4 ? '2/3' : '1',
    context: {width, height, variants: variants.map(({id, colors}) => ({id, colors}))},
  });
}
