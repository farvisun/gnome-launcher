// Create New Image: Raycast's size choices and the pattern generators that
// ImageMagick can draw. Pure: recipes are magick arguments.

export const STANDARD_DIMENSIONS = [1024, 512, 256, 128, 100, 64, 50, 32];

/** Square, Wide, Tall and Extreme sizes, as Raycast's create-image grid groups them. */
export function sizeGroups() {
  const pairs = STANDARD_DIMENSIONS.flatMap(width => STANDARD_DIMENSIONS.map(height => ({width, height})));
  const ratio = ({width, height}) => width / height;
  return [
    {title: 'Square', color: '#8e8e93', sizes: pairs.filter(p => p.width === p.height)},
    {title: 'Wide', color: '#e01b24', sizes: pairs.filter(p => ratio(p) > 4 / 3 && ratio(p) < 15 / 3)},
    {title: 'Tall', color: '#2ec27e', sizes: pairs.filter(p => 1 / ratio(p) > 4 / 3 && 1 / ratio(p) < 15 / 3)},
    {title: 'Extreme', color: '#3584e4', sizes: pairs.filter(p => 1 / ratio(p) > 15 / 3 || ratio(p) > 15 / 3)},
  ];
}

/** A preview of a size: its outline, scaled into a square tile, in the group's colour. */
export function sizeThumbnail({width, height}, color, tile = 96) {
  const scale = (tile - 16) / Math.max(width, height);
  const w = Math.max(2, Math.round(width * scale));
  const h = Math.max(2, Math.round(height * scale));
  return ['-size', `${tile}x${tile}`, 'xc:none', '(', '-size', `${w}x${h}`, `xc:${color}`, '-alpha', 'set', '-channel', 'A', '-evaluate', 'set', '55%', '+channel', ')', '-gravity', 'center', '-composite'];
}

const PALETTE = ['#e01b24', '#ff7800', '#f6d32d', '#33d17a', '#3584e4', '#9141ac', '#865e3c', '#000000', '#ffffff', '#62a0ea', '#f66151', '#57e389', '#dc8add', '#ffbe6f'];
/** Two different colours from the palette, picked by `random` (0–1). */
export function colorPair(random = Math.random) {
  const first = PALETTE[Math.floor(random() * PALETTE.length)];
  let second = first;
  while (second === first) second = PALETTE[Math.floor(random() * PALETTE.length)];
  return [first, second];
}

/** Generators ImageMagick draws faithfully; each takes the size and two colours. */
export const PATTERNS = [
  {id: 'checkerboard', name: 'Checkerboard', args: (w, h, [a, b]) => {
    const square = Math.max(1, Math.round(Math.max(w, h) / 8));
    return ['-size', '2x2', `xc:${a}`, '-fill', b, '-draw', 'point 1,0', '-draw', 'point 0,1', '-scale', `${square * 2}x${square * 2}`,
      '-write', 'mpr:tile', '+delete', '-size', `${w}x${h}`, 'tile:mpr:tile'];
  }},
  {id: 'constant-color', name: 'Constant Color', args: (w, h, [a]) => ['-size', `${w}x${h}`, `xc:${a}`]},
  {id: 'linear-gradient', name: 'Linear Gradient', args: (w, h, [a, b]) => ['-size', `${w}x${h}`, `gradient:${a}-${b}`]},
  {id: 'radial-gradient', name: 'Radial Gradient', args: (w, h, [a, b]) => ['-size', `${w}x${h}`, `radial-gradient:${a}-${b}`]},
  {id: 'random', name: 'Random', args: (w, h) => ['-size', `${w}x${h}`, 'xc:gray', '-attenuate', '1.5', '+noise', 'Random']},
  {id: 'stripes', name: 'Stripes', args: (w, h, [a, b]) => {
    const stripe = Math.max(1, Math.round(Math.max(w, h) / 16));
    return ['-size', `${stripe}x1`, `xc:${a}`, '-size', `${stripe}x1`, `xc:${b}`, '+append', '-write', 'mpr:stripe', '+delete',
      '-size', `${w}x${h}`, 'tile:mpr:stripe'];
  }},
];
/** How many colour variants of each pattern the grid shows, as Raycast does. */
export const VARIANTS = 3;
