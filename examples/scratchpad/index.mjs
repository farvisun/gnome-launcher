import {serve, form, detail, host} from '../../sdk/index.mjs';
serve({note: {
  run: async () => form('Quick note', [{id: 'note', label: 'Your note', type: 'text', required: true, value: await host.storage.get('note') ?? ''}, {id: 'copy', label: 'Copy to clipboard', type: 'boolean', value: true}], 'Save note'),
  submit: async ({values}) => {
    await host.storage.set('note', values.note);
    if (values.copy) await host.copy(values.note);
    await host.notify('Note saved');
    return detail('Saved note', values.note);
  },
}});
