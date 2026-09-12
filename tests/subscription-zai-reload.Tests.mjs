import assert from 'node:assert/strict';
import { isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';

const packageRoot = process.argv[2];
assert.ok(packageRoot && isAbsolute(packageRoot));
const { findLiveProxy } = await import(pathToFileURL(join(packageRoot, 'src/server/proxy-liveness.ts')).href);
const { requestBoundLocalProviderReload } = await import(pathToFileURL(join(packageRoot, 'src/server/local-provider-reload-client.ts')).href);
const live = await findLiveProxy();
assert.ok(live && live.port === 10100 && live.pid, 'live OpenCodex proxy on 10100 is required');
const reload = await requestBoundLocalProviderReload(live, 'zai');
const models = await fetch('http://127.0.0.1:10100/v1/models', { signal: AbortSignal.timeout(15000) }).then(r => r.json());
const ids = (models.data ?? []).map(row => row.id);
const glm = (models.data ?? []).find(row => row.id === 'zai/glm-5.3');
let requestStatus = null;
let requestCode = null;
let requestHost = null;
try {
  const response = await fetch('http://127.0.0.1:10100/v1/responses', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: AbortSignal.timeout(20000),
    body: JSON.stringify({ model: 'zai/glm-5.3', input: 'Reply with exactly OPENCODEX_ZAI_OK.', max_output_tokens: 32, stream: false }),
  });
  const body = await response.json().catch(() => ({}));
  requestStatus = response.status;
  requestCode = body.error?.code || body.error?.type || body.model || null;
} catch (error) {
  requestCode = error instanceof Error ? error.name : 'request-failed';
}
console.log(JSON.stringify({
  pid: live.pid,
  reload,
  hasZai: ids.includes('zai/glm-5.3'),
  hasAstra: ids.includes('gpt-6-astra'),
  hasGrok: ids.includes('xai/grok-4.6'),
  extraGlm: ids.filter(id => id.includes('glm') && id !== 'zai/glm-5.3'),
  glmTool: glm?.tool_mode ?? null,
  requestStatus,
  requestCode,
}));
