import {serve, list, host} from '../../sdk/index.mjs';

const links = [
  {id: 'gnome', title: 'GNOME', subtitle: 'gnome.org', url: 'https://www.gnome.org/'},
  {id: 'handbook', title: 'GNOME Developer Handbook', subtitle: 'developer.gnome.org', url: 'https://developer.gnome.org/'},
  {id: 'extensions', title: 'GNOME Shell Extensions', subtitle: 'extensions.gnome.org', url: 'https://extensions.gnome.org/'},
];
const results = ({query = ''}) => list(links.filter(link => `${link.title} ${link.subtitle}`.toLowerCase().includes(query.toLowerCase())).map(({url, ...link}) => ({...link, icon: 'web-browser-symbolic', actions: [{id: 'primary', title: 'Open in browser'}, {id: 'copy', title: 'Copy URL'}]})));
serve({links: {run: results, query: results, action: async ({item, action}) => {
  const link = links.find(link => link.id === item); if (!link) throw new Error('Link no longer exists');
  if (action === 'copy') await host.copy(link.url); else await host.open(link.url);
}}});
