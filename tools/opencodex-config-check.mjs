// Read-only source validation under the pinned Bun and bounded process runner.
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageRoot, sourceRoot] = process.argv.slice(2);
try {
  assert.ok(isAbsolute(packageRoot) && isAbsolute(sourceRoot));
  const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
  assert.equal(metadata.name, '@bitkyc08/opencodex');
  assert.equal(metadata.version, '2.44.0');
  const config = JSON.parse(readFileSync(join(sourceRoot, 'global/opencodex/config.json'), 'utf8'));
  const secretFields = /^(api[-_]?keys?|access[-_]?token|refresh[-_]?token|password(?:hash)?|auth[-_]?token|authorization|cookie)$/i;
  function inspect(value) {
    if (!value || typeof value !== 'object') return;
    for (const [key, child] of Object.entries(value)) {
      assert.ok(!secretFields.test(key), 'Credential-bearing fields belong to private host state');
      inspect(child);
    }
  }
  inspect(config);
  const { validateConfigCandidate } = await import(pathToFileURL(join(packageRoot, 'src/config.ts')).href);
  assert.equal(validateConfigCandidate(config).ok, true, 'OpenCodex source configuration rejected by pinned schema');
  assert.equal(config.hostname, '127.0.0.1');
  assert.equal(config.providers.xai.authMode, 'oauth');
  const search = config.webSearchSidecar;
  assert.ok(search?.enabled === false || (search?.backend === 'xai' && search?.model === 'grok-4.6'), 'Search must not implicitly borrow OpenAI quota');
  assert.equal(config.visionSidecar?.enabled, false, 'Implicit vision helper must remain disabled');
  assert.ok((config.subagentModels ?? []).every(model => model.startsWith('gpt-6-astra') || model.startsWith('xai/')));
  const rolesDirectory = join(sourceRoot, 'global/opencodex/agents');
  const roles = [];
  for (const filename of readdirSync(rolesDirectory).filter(name => name.endsWith('.toml'))) {
    const role = Bun.TOML.parse(readFileSync(join(rolesDirectory, filename), 'utf8'));
    inspect(role);
    for (const field of ['name', 'description', 'developer_instructions', 'model', 'model_provider']) assert.ok(typeof role[field] === 'string' && role[field].trim());
    assert.equal(role.model_provider, 'openai');
    assert.ok(!Object.hasOwn(role, 'model_fallback'));
    assert.ok(role.model.includes('/'));
    assert.ok(!roles.includes(role.name));
    roles.push(role.name);
    if (role.name === 'middle') {
      assert.equal(role.model, 'xai/grok-4.6');
      assert.equal(role.model_reasoning_effort, 'xhigh');
      assert.equal(role.agents?.enabled, false);
    }
  }
  assert.ok(roles.includes('middle'));
  console.log(JSON.stringify({ valid: true, packageVersion: metadata.version, roles }));
} catch (error) {
  // A parse/schema error can quote a supplied value. Keep failures value-free.
  console.error(`Subscription source validation failed (${error?.name ?? 'Error'}).`);
  process.exitCode = 1;
}
