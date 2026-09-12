import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const packageRoot = process.argv[2];
const keyPath = process.argv[3];
assert.ok(packageRoot && isAbsolute(packageRoot));
assert.ok(keyPath && isAbsolute(keyPath));
const key = readFileSync(keyPath, 'utf8').trim();
assert.ok(key && !/[\r\n]/.test(key));
process.env.ZAI_API_KEY = key;
const source = JSON.parse(readFileSync(join(process.cwd(), 'global/opencodex/config.json'), 'utf8'));
const root = mkdtempSync(join(tmpdir(), 'codex-zai-req-'));
const ocxHome = join(root, 'opencodex');
const codexHome = join(root, 'codex');
mkdirSync(ocxHome); mkdirSync(codexHome);
process.env.OPENCODEX_HOME = ocxHome;
process.env.CODEX_HOME = codexHome;
const config = structuredClone(source);
config.port = 10103;
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
    upstream.push({ host: url.hostname, path: url.pathname, method: init?.method || 'GET' });
  }
  return originalFetch(input, init);
};
const { validateConfigCandidate } = await import(pathToFileURL(join(packageRoot, 'src/config.ts')).href);
assert.equal(validateConfigCandidate(config).ok, true);
const { startServer } = await import(pathToFileURL(join(packageRoot, 'src/server/index.ts')).href);
const server = startServer(10103);
try {
  const models = await fetch('http://127.0.0.1:10103/v1/models', { signal: AbortSignal.timeout(15000) }).then(r => r.json());
  const ids = (models.data ?? []).map(row => row.id);
  assert.ok(ids.includes('zai/glm-5.3'));
  assert.ok(ids.includes('gpt-6-astra'));
  assert.ok(ids.includes('xai/grok-4.6'));
  assert.deepEqual([...ids].sort(), ['gpt-6-astra', 'xai/grok-4.6', 'zai/glm-5.3']);
  const response = await fetch('http://127.0.0.1:10103/v1/responses', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: AbortSignal.timeout(45000),
    body: JSON.stringify({ model: 'zai/glm-5.3', input: 'Reply with exactly OPENCODEX_ZAI_OK.', max_output_tokens: 32, stream: false }),
  });
  const body = await response.json().catch(() => ({}));
  const zaiCalls = upstream.filter(row => row.host === 'api.z.ai');
  const chat = zaiCalls.some(row => row.path.includes('/api/coding/paas/v4'));
  const responsesV1 = zaiCalls.some(row => row.path.includes('/api/v1'));
  const xaiCalls = upstream.filter(row => row.host.includes('grok') || row.host.includes('x.ai'));
  console.log(JSON.stringify({
    ok: response.ok,
    status: response.status,
    model: body.model || null,
    hasZai: ids.includes('zai/glm-5.3'),
    chatPath: chat,
    responsesV1,
    xaiCalls: xaiCalls.length,
    zaiPaths: zaiCalls.map(row => row.path),
    error: body.error ? { code: body.error.code || null, type: body.error.type || null } : null,
  }));
  assert.equal(xaiCalls.length, 0);
  assert.equal(responsesV1, false);
  assert.equal(chat, true);
} finally {
  await server?.stop?.();
  rmSync(root, { recursive: true, force: true });
}
