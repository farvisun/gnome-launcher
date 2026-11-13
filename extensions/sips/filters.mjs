// Raycast's Core Image filters that ImageMagick can render convincingly, with
// the same names, categories and descriptions. Each recipe gives the magick
// arguments between the input and the output; `w` and `h` are the image size.

const disk = r => ['-define', 'convolve:scale=!', '-morphology', 'Convolve', `Disk:${r}`];
const tile = (w, h) => ['-resize', `${w}x${h}!`];

export const FILTERS = [
  // Blur
  {name: 'Bokeh Blur', category: 'Blur', description: 'Applies a bokeh effect, like an out-of-focus camera lens.', args: ({w}) => [...disk(Math.max(2, Math.round(w / 60))), '-modulate', '104,108']},
  {name: 'Box Blur', category: 'Blur', description: 'Blur effect using a box-shaped convolution kernel.', args: ({w}) => ['-define', 'convolve:scale=!', '-morphology', 'Convolve', `Square:${Math.max(2, Math.round(w / 80))}`]},
  {name: 'Disc Blur', category: 'Blur', description: 'Blur effect that uses a disc-shaped convolution kernel.', args: ({w}) => disk(Math.max(2, Math.round(w / 90)))},
  {name: 'Gaussian Blur', category: 'Blur', description: 'Spreads source pixels by an amount specified by a Gaussian distribution.', args: ({w}) => ['-gaussian-blur', `0x${Math.max(1, Math.round(w / 100))}`]},
  {name: 'Median', category: 'Blur', description: 'Replaces each pixel with the median of its neighbourhood, reducing noise.', args: () => ['-statistic', 'Median', '5x5']},
  {name: 'Motion Blur', category: 'Blur', description: 'Blur effect that simulates the effect of using a camera that moves a specified angle and distance while capturing the image.', args: ({w}) => ['-motion-blur', `0x${Math.max(2, Math.round(w / 40))}+0`]},
  {name: 'Noise Reduction', category: 'Blur', description: 'Reduces noise by smoothing small changes in luminance while sharpening edges.', args: () => ['-despeckle', '-despeckle', '-unsharp', '0x1+0.5+0.02']},
  // Color Effect
  {name: 'Chrome', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates vintage photography film with exaggerated color.', args: () => ['-sigmoidal-contrast', '4,50%', '-modulate', '100,125']},
  {name: 'Dither', category: 'Color Effect', description: 'Applies randomized noise to produce a processed look.', args: () => ['-ordered-dither', 'o4x4,6']},
  {name: 'Document Enhancement', category: 'Color Effect', description: 'Removes unwanted shadows, whitens the background, and enhances contrast of a scanned document.', args: () => ['-normalize', '-level', '10%,88%', '-unsharp', '0x1.5+1+0.02']},
  {name: 'Fade', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates vintage photography film with diminished color.', args: () => ['-modulate', '100,65', '+level', '12%,94%']},
  {name: 'Instant', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates vintage photography film with distorted colors.', args: () => ['-modulate', '103,85', '-fill', '#f4dcb0', '-colorize', '14%', '+level', '4%,98%']},
  {name: 'Invert', category: 'Color Effect', description: 'Inverts the colors in an image.', args: () => ['-negate']},
  {name: 'Maximum Component', category: 'Color Effect', description: 'Converts an image to grayscale using the maximum of its red, green and blue values.', args: () => ['-grayscale', 'Brightness']},
  {name: 'Minimum Component', category: 'Color Effect', description: 'Converts an image to grayscale using the minimum of its red, green and blue values.', args: () => ['-negate', '-grayscale', 'Brightness', '-negate']},
  {name: 'Mono', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates black-and-white photography film with low contrast.', args: () => ['-colorspace', 'Gray', '+level', '6%,96%']},
  {name: 'Noir', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates black-and-white photography film with exaggerated contrast.', args: () => ['-colorspace', 'Gray', '-sigmoidal-contrast', '6,50%']},
  {name: 'Posterize', category: 'Color Effect', description: 'Remaps red, green, and blue color components to the number of brightness values you specify for each color component.', args: () => ['-posterize', '6']},
  {name: 'Process', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates vintage photography film with emphasized cool colors.', args: () => ['-modulate', '100,85', '-fill', '#5f8fb0', '-tint', '55', '+level', '4%,100%']},
  {name: 'Sepia', category: 'Color Effect', description: 'Maps the colors of an image to various shades of brown.', args: () => ['-modulate', '100,20', '-fill', '#704214', '-tint', '90']},
  {name: 'Thermal', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates a thermal camera.', args: () => ['-colorspace', 'Gray', '(', 'xc:#0a0050', 'xc:#6a00c8', 'xc:#e8003a', 'xc:#ff9b00', 'xc:#fff35a', '+append', '-filter', 'Cubic', '-resize', '256x1!', ')', '-clut']},
  {name: 'Tonal', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates black-and-white photography film without significantly altering contrast.', args: () => ['-colorspace', 'Gray']},
  {name: 'Transfer', category: 'Color Effect', description: 'Applies a preconfigured set of effects that imitate vintage photography film with emphasized warm colors.', args: () => ['-modulate', '102,95', '-fill', '#ffb347', '-colorize', '12%', '+level', '3%,100%']},
  {name: 'Vignette', category: 'Color Effect', description: 'Applies a vignette shading to the corners of an image.', args: ({w, h}) => ['(', '-size', `${w}x${h}`, 'radial-gradient:white-black', '-evaluate', 'pow', '0.55', '+level', '18%,100%', ')', '-compose', 'Multiply', '-composite']},
  {name: 'X-Ray', category: 'Color Effect', description: 'Applies a preconfigured effect that imitates an X-ray image.', args: () => ['-colorspace', 'Gray', '-negate', '-level', '8%,92%', '-fill', '#b7d3ff', '-tint', '70']},
  // Halftone
  {name: 'Circular Screen', category: 'Halftone', description: 'Simulates a circular-shaped halftone screen.', args: () => ['-colorspace', 'Gray', '-ordered-dither', 'c7x7b']},
  {name: 'Dot Screen', category: 'Halftone', description: 'Simulates the dot patterns of a halftone screen.', args: () => ['-colorspace', 'Gray', '-ordered-dither', 'h6x6a']},
  {name: 'CMYK Halftone', category: 'Halftone', description: 'Creates a color, halftoned rendition of the source image, using cyan, magenta, yellow, and black inks over a white page.', args: () => ['-colorspace', 'CMYK', '-ordered-dither', 'h6x6a', '-colorspace', 'sRGB']},
  // Distortion
  {name: 'Bump', category: 'Distortion', description: 'Creates a concave or convex bump that originates at a specified point in the image.', args: () => ['-implode', '-0.6']},
  {name: 'Circular Wrap', category: 'Distortion', description: 'Wraps an image around a transparent circle.', args: ({w, h}) => ['-background', 'none', '-virtual-pixel', 'Background', '-distort', 'Arc', '360', '-resize', `${w}x${h}`]},
  {name: 'Pinch', category: 'Distortion', description: 'Creates a rectangular-shaped area that pinches source pixels inward, distorting those pixels closest to the rectangle the most.', args: () => ['-implode', '0.6']},
  {name: 'Twirl', category: 'Distortion', description: 'Rotates pixels around a point to give a twirling effect.', args: () => ['-swirl', '180']},
  {name: 'Vortex', category: 'Distortion', description: 'Rotates pixels around a point to simulate a vortex.', args: () => ['-swirl', '540']},
  // Sharpen
  {name: 'Sharpen Luminance', category: 'Sharpen', description: 'Increases image detail by sharpening.', args: () => ['-colorspace', 'Lab', '-channel', 'R', '-sharpen', '0x2', '+channel', '-colorspace', 'sRGB']},
  {name: 'Unsharp Mask', category: 'Sharpen', description: 'Increases the contrast of the edges between pixels of different colors in an image.', args: () => ['-unsharp', '0x3+1.5+0.02']},
  // Stylize
  {name: 'Bloom', category: 'Stylize', description: 'Softens edges and applies a pleasant glow to an image.', args: ({w}) => ['(', '+clone', '-gaussian-blur', `0x${Math.max(2, Math.round(w / 40))}`, '-evaluate', 'multiply', '0.6', ')', '-compose', 'Screen', '-composite']},
  {name: 'Comic', category: 'Stylize', description: 'Simulates a comic book drawing by outlining edges and applying a color halftone effect.', args: () => ['(', '+clone', '-posterize', '5', ')', '(', '-clone', '0', '-colorspace', 'Gray', '-canny', '0x1+10%+30%', '-negate', ')', '-delete', '0', '-compose', 'Multiply', '-composite']},
  {name: 'Edges', category: 'Stylize', description: 'Finds all edges in an image and displays them in color.', args: () => ['-edge', '1', '-normalize']},
  {name: 'Edge Work', category: 'Stylize', description: 'Produces a stylized black-and-white rendition of an image that looks similar to a woodblock cutout.', args: () => ['-colorspace', 'Gray', '-canny', '0x1+10%+25%']},
  {name: 'Gloom', category: 'Stylize', description: 'Dulls the highlights of an image.', args: ({w}) => ['(', '+clone', '-gaussian-blur', `0x${Math.max(2, Math.round(w / 40))}`, ')', '-compose', 'Darken', '-composite']},
  {name: 'Line Overlay', category: 'Stylize', description: 'Creates a sketch that outlines the edges of an image in black.', args: () => ['-colorspace', 'Gray', '-sketch', '0x8+120']},
  {name: 'Pixellate', category: 'Stylize', description: 'Makes an image blocky by mapping the image to colored squares whose color is defined by the replaced pixels.', args: ({w, h}) => {
    // Blocks of at least 8 px, or about a hundredth of the width on large images.
    const block = Math.max(8, Math.round(w / 100));
    return ['-scale', `${Math.max(1, Math.round(w / block))}x${Math.max(1, Math.round(h / block))}!`, '-scale', `${w}x${h}!`];
  }},
  {name: 'Spotlight', category: 'Stylize', description: 'Applies a directional spotlight effect to an image.', args: ({w, h}) => ['(', '-size', `${w}x${h}`, 'xc:black', '-fill', 'white', '-draw', `ellipse ${Math.round(w * 0.45)},${Math.round(h * 0.4)} ${Math.round(w * 0.35)},${Math.round(h * 0.35)} 0,360`, '-gaussian-blur', `0x${Math.max(2, Math.round(w / 12))}`, ')', '-compose', 'Multiply', '-composite']},
  // Tile
  {name: 'Fourfold Reflected Tile', category: 'Tile', description: 'Produces a tiled image from a source image by applying a 4-way reflected symmetry.', args: ({w, h}) => ['-resize', `${Math.ceil(w / 2)}x${Math.ceil(h / 2)}!`, '(', '+clone', '-flop', ')', '+append', '(', '+clone', '-flip', ')', '-append', ...tile(w, h)]},
  {name: 'Fourfold Translated Tile', category: 'Tile', description: 'Produces a tiled image from a source image by applying 4 translation operations.', args: ({w, h}) => ['-resize', `${Math.ceil(w / 2)}x${Math.ceil(h / 2)}!`, '(', '+clone', ')', '+append', '(', '+clone', ')', '-append', ...tile(w, h)]},
].map(filter => ({...filter, id: filter.name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}));

/** Filters not hidden by name or category in the comma-separated `hiddenFilters` preference. */
export function activeFilters(hidden = '') {
  const names = new Set(String(hidden).split(',').map(n => n.trim().toLowerCase()).filter(Boolean));
  return FILTERS.filter(f => !names.has(f.name.toLowerCase()) && !names.has(f.category.toLowerCase()));
}
