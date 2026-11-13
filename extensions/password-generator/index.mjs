import {serve, form, host} from '../../sdk/index.mjs';
import {FORMAT_HELP, generateCustomPassword, generatePassword, parseLength} from './lib.mjs';

/** Copies the password, then shows a toast, or closes Lumen with a HUD. */
async function deliver(password, preferences, note = '') {
  await host.copy(password);
  if (preferences.hideAfterCopy) {
    await host.hud(`Copied Password - ${password} 🎉`);
  } else {
    await host.toast({style: 'success', title: 'Copied Password 🎉', message: password + note});
  }
}

const stored = async key => { try { return await host.storage.get(key); } catch { return null; } };

async function randomForm({preferences}) {
  const length = preferences.storePasswordLength ? await stored('length') : null;
  const options = preferences.rememberNumbersAndCharacters ? await stored('options') : null;
  return form('Password Generator', [
    {id: 'length', label: 'Number of characters', placeholder: 'Enter a number between 5 and 64', value: length ?? ''},
    {id: 'useNumbers', label: 'Use numbers?', type: 'boolean', value: options?.useNumbers ?? true},
    {id: 'useChars', label: 'Use special characters?', type: 'boolean', value: options?.useChars ?? true},
  ], 'Generate Password');
}

async function memorableForm({preferences}) {
  const format = preferences.rememberCustomFormat ? await stored('customFormat') : null;
  return form('Password Generator', [
    {id: 'customFormat', label: 'Custom password format', placeholder: 'e.g. {word} {word:uppercase} {symbol:2} {number:4}', value: format ?? ''},
    {id: 'help', label: 'Format options', type: 'description', text: FORMAT_HELP},
  ], 'Generate Password');
}

serve({
  'generate-random-password': {
    run: randomForm,
    submit: async ({values, preferences}) => {
      const {length, error} = parseLength(values.length);
      if (error) {
        await host.toast({style: 'failure', title: error});
        return null;
      }
      if (preferences.storePasswordLength) await host.storage.set('length', String(length));
      if (preferences.rememberNumbersAndCharacters) await host.storage.set('options', {useNumbers: values.useNumbers, useChars: values.useChars});
      await deliver(generatePassword(length, values.useNumbers, values.useChars), preferences);
      return null;
    },
  },
  'copy-random-password': {
    run: async ({preferences}) => {
      const {length, error} = parseLength(preferences.length);
      if (error) {
        await host.toast({style: 'failure', title: error});
        return null;
      }
      const password = generatePassword(length, preferences.useNumbers, preferences.useChars);
      await host.copy(password);
      await host.hud(preferences.hideAfterCopy ? `Copied Password - ${password} 🎉` : 'Copied Password 🎉');
      return null;
    },
  },
  'generate-memorable-password': {
    run: memorableForm,
    submit: async ({values, preferences}) => {
      const format = String(values.customFormat ?? '');
      if (!format.trim()) {
        await host.toast({style: 'failure', title: 'Enter a password format'});
        return null;
      }
      if (preferences.rememberCustomFormat) await host.storage.set('customFormat', format);
      const warning = format.trim() !== format ? ' (Warning: your password contains leading or trailing whitespace)' : '';
      await deliver(generateCustomPassword(format), preferences, warning);
      return null;
    },
  },
});
