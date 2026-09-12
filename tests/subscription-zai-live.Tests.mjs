import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const packageRoot = process.argv[2];
assert.ok(packageRoot && isAbsolute(packageRoot));
const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
assert.equal(metadata.name, '@bitkyc08/opencodex');
assert.equal(metadata.version, '2.44.0');
const source = JSON.parse(readFileSync(join(process.cwd(), 'global/opencodex/config.json'), 'utf8'));
const root = mkdtempSync(join(tmpdir(), 'codex-zai-live-'));
const ocxHome = join(root, 'opencodex');
const codexHome = join(root, 'codex');
await import('node:fs/promises').then(fs => Promise.all([fs.mkdir(ocxHome), fs.mkdir(codexHome)]));
process.env.OPENCODEX_HOME = ocxHome;
process.env.CODEX_HOME = codexHome;
const config = structuredClone(source);
config.port = 10102;
config.clientIntegrations = { codex: false, grok: false, 'claude-desktop': false };
config.webSearchSidecar = { enabled: false };
config.visionSidecar = { enabled: false };
config.codexAutoStart = false;
config.codexShimAutoRestore = false;
config.providers.xai.liveModels = false;
config.providers.zai.liveModels = false;
writeFileSync(join(ocxHome, 'config.json'), JSON.stringify(config, null, 2));
const originalFetch = globalThis.fetch;
const upstream = [];
globalThis.fetch = async (input, init) => {
  const url = new URL(typeof input === 'string' || input instanceof URL ? input : input.url);
  if (url.hostname !== '127.0.0.1') {
    upstream.push({ host: url.hostname, path: url.pathname });
    throw new Error('isolated zai probe forbids external requests');
  }
  return originalFetch(input, init);
};
const { validateConfigCandidate } = await import(pathToFileURL(join(packageRoot, 'src/config.ts')).href);
assert.equal(validateConfigCandidate(config).ok, true);
const { startServer } = await import(pathToFileURL(join(packageRoot, 'src/server/index.ts')).href);
const server = startServer(10102);
try {
  const health = await fetch('http://127.0.0.1:10102/healthz', { signal: AbortSignal.timeout(5000) }).then(r => r.json());
  assert.equal(health.service, 'opencodex');
  const models = await fetch('http://127.0.0.1:10102/v1/models', { signal: AbortSignal.timeout(15000) }).then(r => r.json());
  const ids = (models.data ?? []).map(row => row.id);
  assert.ok(ids.includes('gpt-6-astra'));
  assert.ok(ids.includes('xai/grok-4.6'));
  assert.ok(ids.includes('zai/glm-5.3'));
  assert.deepEqual([...ids].sort(), ['gpt-6-astra', 'xai/grok-4.6', 'zai/glm-5.3']);
  assert.ok(!ids.some(id => id.includes('glm-5.2') || id.includes('flash') || id.includes('[1m]')));
  const glm = (models.data ?? []).find(row => row.id === 'zai/glm-5.3');
  // /v1/models rows never expose tool_mode; the on-disk catalogue row owns
  // the Code Mode advertisement (covered by subscription-zai-catalog.Tests.mjs).
  assert.equal(glm?.tool_mode, undefined);
  const astra = (models.data ?? []).find(row => row.id === 'gpt-6-astra');
  const response = await fetch('http://127.0.0.1:10102/v1/responses', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: AbortSignal.timeout(10000),
    body: JSON.stringify({ model: 'zai/glm-5.3', input: 'Reply OK.', max_output_tokens: 16, stream: false }),
  });
  const body = await response.json();
  assert.ok(response.status >= 400 && response.status < 600);
  assert.ok(body.error && !body.output);
  assert.equal(upstream.length, 0);
  console.log(JSON.stringify({ ok: true, ids: ids.filter(id => /astra|grok-4.6|glm/.test(id)), status: response.status, code: body.error?.code || body.error?.type || null, astraTool: astra?.tool_mode ?? null, glmTool: glm?.tool_mode ?? null }));
} finally {
  await server?.stop?.();
  rmSync(root, { recursive: true, force: true });
}
