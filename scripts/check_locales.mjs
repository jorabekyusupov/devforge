import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';

function flatten(value, prefix = '', output = {}) {
  for (const [key, child] of Object.entries(value)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof child === 'string' && child.trim()) output[path] = child;
    else if (child && typeof child === 'object' && !Array.isArray(child)) flatten(child, path, output);
    else throw new Error(`Translation must be a non-empty string: ${path}`);
  }
  return output;
}
const baseKey = key => key.replace(/_(zero|one|two|few|many|other)$/, '');
const parameters = message => [...message.matchAll(/{{\s*([\w.]+)\s*}}/g)].map(match => match[1]).sort().join(',');

export function validateCatalogs(registry, catalogs) {
  if (!registry.locales.length || !registry.locales.some(locale => locale.id === registry.fallback)) throw new Error('Invalid locale fallback');
  const identifiers = new Set();
  for (const locale of registry.locales) {
    if (!locale.name || !['ltr','rtl'].includes(locale.direction) || Intl.getCanonicalLocales(locale.id)[0] !== locale.id) throw new Error(`Invalid locale: ${locale.id}`);
    for (const id of [locale.id, ...(locale.aliases ?? [])]) {
      const normalized = id.toLowerCase();
      if (identifiers.has(normalized)) throw new Error(`Ambiguous locale: ${id}`);
      identifiers.add(normalized);
    }
  }
  const reference = flatten(catalogs[registry.fallback]);
  const expected = new Set(Object.keys(reference).map(baseKey));
  const pluralKeys = new Set(Object.keys(reference).filter(key => /_(zero|one|two|few|many|other)$/.test(key)).map(baseKey));
  for (const locale of registry.locales) {
    if (!catalogs[locale.id]) throw new Error(`Missing catalog: ${locale.id}`);
    const messages = flatten(catalogs[locale.id]);
    const keys = new Set(Object.keys(messages).map(baseKey));
    if ([...expected].some(key => !keys.has(key)) || [...keys].some(key => !expected.has(key))) throw new Error(`Translation coverage differs: ${locale.id}`);
    for (const [key, message] of Object.entries(messages)) {
      const original = Object.entries(reference).find(([candidate]) => baseKey(candidate) === baseKey(key))[1];
      if (parameters(message) !== parameters(original)) throw new Error(`Placeholder mismatch: ${locale.id}/${key}`);
    }
    for (const stem of pluralKeys) {
      for (const category of new Intl.PluralRules(locale.id).resolvedOptions().pluralCategories) {
        if (!messages[`${stem}_${category}`]) throw new Error(`Missing plural category: ${locale.id}/${stem}_${category}`);
      }
    }
  }
  return registry.locales.length;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const directory = resolve(dirname(fileURLToPath(import.meta.url)), '../shared/locales');
  const registry = JSON.parse(readFileSync(resolve(directory,'registry.json'),'utf8'));
  const catalogs = Object.fromEntries(registry.locales.map(locale => [locale.id,JSON.parse(readFileSync(resolve(directory,`${locale.id}.json`),'utf8'))]));
  try { console.log(`Locale check passed: ${validateCatalogs(registry,catalogs)} catalogs, semantic keys, placeholders and plural forms.`); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
