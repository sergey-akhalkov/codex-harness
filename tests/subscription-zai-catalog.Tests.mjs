// Isolated catalogue filter for the reusable zai provider. No network, no live homes.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const packageRoot = process.argv[2];
assert.ok(packageRoot && isAbsolute(packageRoot), 'Supply the pinned OpenCodex package root.');
const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
assert.equal(metadata.name, '@bitkyc08/opencodex');
assert.equal(metadata.version, '2.44.0');
const sourceConfig = JSON.parse(readFileSync(join(process.cwd(), 'global/opencodex/config.json'), 'utf8'));
assert.equal(sourceConfig.providers.zai.defaultModel, 'glm-5.3');
assert.deepEqual(sourceConfig.providers.zai.selectedModels, ['glm-5.3']);
assert.equal(sourceConfig.providers.zai.codexToolMode, undefined);
assert.equal(sourceConfig.defaultProvider, 'openai');
assert.equal(sourceConfig.providers.xai.authMode, 'oauth');
assert.equal(sourceConfig.fastRows, false);
assert.deepEqual(sourceConfig.providers.xai.selectedModels, ['grok-4.6']);
assert.ok(sourceConfig.providers.xai.models.includes('grok-4.6'));
assert.ok((sourceConfig.disabledModels ?? []).includes('gpt-5.6-sol'));
assert.ok(!(sourceConfig.disabledModels ?? []).includes('gpt-6-astra'));
assert.match(sourceConfig.providers.zai.apiKey, /^\$\{ZAI_API_KEY\}$/);

const { filterCatalogVisibleModels } = await import(pathToFileURL(join(packageRoot, 'src/codex/catalog/provider-fetch.ts')).href);
const { deriveEntry } = await import(pathToFileURL(join(packageRoot, 'src/codex/catalog/sync.ts')).href);
const { applyNativeVisibility, visibleNativeSlugs } = await import(pathToFileURL(join(packageRoot, 'src/codex/catalog/metadata.ts')).href);

const models = [
  { provider: 'openai', id: 'gpt-6-astra' },
  { provider: 'openai', id: 'gpt-5.6-sol' },
  { provider: 'xai', id: 'grok-4.6' },
  { provider: 'xai', id: 'grok-4.5' },
  { provider: 'zai', id: 'glm-5.3', reasoningEfforts: ['low', 'high', 'max'], defaultReasoningEffort: 'max', contextWindow: 1000000 },
  { provider: 'zai', id: 'glm-5.2' },
  { provider: 'zai', id: 'glm-5.3-flash' },
  { provider: 'zai', id: 'glm-5.3[1m]' },
];
const visible = filterCatalogVisibleModels(models, sourceConfig);
assert.deepEqual(visible.map(model => model.provider + '/' + model.id), ['openai/gpt-6-astra', 'openai/gpt-5.6-sol', 'xai/grok-4.6', 'zai/glm-5.3']);
const natives = visibleNativeSlugs(sourceConfig);
assert.ok(natives.includes('gpt-6-astra'));
assert.ok(!natives.includes('gpt-5.6-sol'));
assert.ok(!natives.includes('gpt-5.5'));
const nativeRows = applyNativeVisibility(
  [{ slug: 'gpt-6-astra' }, { slug: 'gpt-5.6-sol' }, { slug: 'xai/grok-4.6' }, { slug: 'zai/glm-5.3' }],
  new Set(sourceConfig.disabledModels ?? []),
);
assert.equal(nativeRows.find(row => row.slug === 'gpt-6-astra')?.visibility, 'list');
assert.equal(nativeRows.find(row => row.slug === 'gpt-5.6-sol')?.visibility, 'hide');
assert.equal(nativeRows.find(row => row.slug === 'xai/grok-4.6')?.visibility, undefined);

const entry = deriveEntry(null, 'zai/glm-5.3', 'Z.AI GLM-5.3', 5, {
  provider: 'zai',
  id: 'glm-5.3',
  reasoningEfforts: ['low', 'high', 'max'],
  defaultReasoningEffort: 'max',
  contextWindow: 1000000,
  inputModalities: ['text'],
});
assert.equal(entry.slug, 'zai/glm-5.3');
assert.equal(entry.visibility, 'list');
assert.equal(entry.tool_mode, 'code_mode_only');
assert.ok(!JSON.stringify(sourceConfig).includes('sk-') && sourceConfig.providers.zai.apiKey.startsWith('$'));
console.log(JSON.stringify({ ok: true, slugs: visible.map(model => model.provider + '/' + model.id), glm: entry.slug }));
