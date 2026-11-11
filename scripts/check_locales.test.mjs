import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { validateCatalogs } from './check_locales.mjs';

const root = new URL('../shared/locales/',import.meta.url);
const config = JSON.parse(readFileSync(new URL('registry.json',root)));
const messages = Object.fromEntries(config.locales.map(locale => [locale.id,JSON.parse(readFileSync(new URL(`${locale.id}.json`,root)))]));
test('ships complete catalogs and permits a data-only fourth locale', () => {
  assert.equal(validateCatalogs(config,messages),3);
  const expanded={...config,locales:[...config.locales,{id:'de',name:'Deutsch',direction:'ltr',aliases:[]}]};
  assert.equal(validateCatalogs(expanded,{...messages,de:messages.en}),4);
});
test('rejects lost keys, renamed placeholders and absent Russian plural forms', () => {
  for (const breakCatalog of [
    value => delete value.ru.native.quit,
    value => value.ru.settings.languageCount_one='{{total}} язык',
    value => delete value.ru.settings.languageCount_other,
  ]) {
    const broken=structuredClone(messages);breakCatalog(broken);
    assert.throws(() => validateCatalogs(config,broken));
  }
});
test('rejects duplicate locale aliases', () => {
  const broken=structuredClone(config);broken.locales[1].aliases.push('en');
  assert.throws(() => validateCatalogs(broken,messages),/Ambiguous/);
});
