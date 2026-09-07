// Opt-in finite integration probe. Run only inside opencodex-process.ps1 with
// a 768 MiB job and 120-second timeout. No daemon, CLI injection or native home.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [optIn, packageRoot, reportPath] = process.argv.slice(2);
const failureProbe = optIn === '--RunFailureProbes';
if (optIn !== '--RunModelProbes' && !failureProbe) {
  console.log('SKIP: explicit --RunModelProbes and isolated runtime homes required.');
  process.exit(0);
}
assert.ok(isAbsolute(packageRoot) && isAbsolute(reportPath));
const ocxHome = process.env.OPENCODEX_HOME;
const codexHome = process.env.CODEX_HOME;
assert.ok(ocxHome && codexHome && isAbsolute(ocxHome) && isAbsolute(codexHome));
assert.notEqual(resolve(codexHome).toLowerCase(), resolve(process.env.USERPROFILE, '.codex').toLowerCase());
assert.notEqual(resolve(ocxHome).toLowerCase(), resolve(process.env.USERPROFILE, '.opencodex').toLowerCase());
const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
assert.equal(metadata.name, '@bitkyc08/opencodex');
assert.equal(metadata.version, '2.44.0');
const config = JSON.parse(readFileSync(join(ocxHome, 'config.json'), 'utf8'));
assert.equal(config.hostname, '127.0.0.1');
assert.equal(config.port, 10101);
for (const client of ['codex', 'grok', 'claude-desktop']) assert.equal(config.clientIntegrations[client], false);
assert.equal(config.webSearchSidecar.enabled, false);
assert.equal(config.visionSidecar.enabled, false);
assert.equal(config.providers.xai.authMode, 'oauth');
if (failureProbe) assert.equal(existsSync(join(ocxHome, 'auth.json')), false, 'Failure fixture must have no credentials');
const report = { status: 'running', scenario: failureProbe ? 'missing-auth-and-provider' : 'live-subscription', packageVersion: metadata.version, startedAt: new Date().toISOString(), upstream: [] };
const originalFetch = globalThis.fetch;
globalThis.fetch = async (input, init) => {
  const url = new URL(typeof input === 'string' || input instanceof URL ? input : input.url);
  if (failureProbe && url.hostname !== '127.0.0.1') {
    report.upstream.push({ host: url.hostname, path: url.pathname, blocked: true });
    throw new Error('Failure fixture forbids external requests');
  }
  assert.ok(['127.0.0.1', 'cli-chat-proxy.grok.com', 'auth.x.ai'].includes(url.hostname), 'Unexpected probe destination');
  const response = await originalFetch(input, { ...init, redirect: 'error' });
  if (url.hostname !== '127.0.0.1') report.upstream.push({ host: url.hostname, path: url.pathname, status: response.status });
  return response;
};
let server;
try {
  const { validateConfigCandidate } = await import(pathToFileURL(join(packageRoot, 'src/config.ts')).href);
  assert.equal(validateConfigCandidate(config).ok, true);
  const { startServer } = await import(pathToFileURL(join(packageRoot, 'src/server/index.ts')).href);
  server = startServer(10101);
  const health = await fetch('http://127.0.0.1:10101/healthz', { signal: AbortSignal.timeout(5000) }).then(r => r.json());
  assert.equal(health.pid, process.pid);
  assert.equal(health.service, 'opencodex');
  report.health = { status: health.status, pid: health.pid, version: health.version };
  if (failureProbe) {
    report.failures = [];
    for (const model of ['xai/grok-4.6', 'unavailable_fixture/grok-4.6']) {
      const response = await fetch('http://127.0.0.1:10101/v1/responses', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: AbortSignal.timeout(10000),
        body: JSON.stringify({ model, input: 'Reply OK.', max_output_tokens: 16, stream: false }),
      });
      const body = await response.json();
      report.failures.push({ model, status: response.status, code: body.error?.code, type: body.error?.type });
      assert.ok(response.status >= 400 && response.status < 600);
      assert.ok(body.error && !body.output);
    }
    assert.equal(report.upstream.length, 0, 'Auth/model failure must not dispatch to another route');
  } else {
  const models = await fetch('http://127.0.0.1:10101/v1/models', { signal: AbortSignal.timeout(30000) }).then(r => r.json());
  report.models = models.data.filter(row => row.id.startsWith('xai/')).map(row => row.id);
  assert.ok(report.models.includes('xai/grok-4.6'));
  const response = await fetch('http://127.0.0.1:10101/v1/responses', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, signal: AbortSignal.timeout(45000),
    body: JSON.stringify({ model: 'xai/grok-4.6', input: 'Reply with exactly PROXY_SUBSCRIPTION_OK.', reasoning: { effort: 'high' }, max_output_tokens: 128, stream: false }),
  });
  report.responseStatus = response.status;
  const body = await response.json();
  report.model = body.model;
  report.answer = (body.output ?? []).flatMap(item => item.content ?? []).filter(item => item.type === 'output_text').map(item => item.text).join('');
  if (!response.ok) report.errorCode = body.error?.code;
  assert.equal(response.status, 200);
  assert.ok(report.answer.includes('PROXY_SUBSCRIPTION_OK'));
  assert.ok(report.upstream.some(row => row.host === 'cli-chat-proxy.grok.com' && row.path.endsWith('/responses') && row.status === 200));
  }
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.errorType = error?.name ?? 'Error';
  process.exitCode = 1;
} finally {
  if (server) await server.stop(true);
  globalThis.fetch = originalFetch;
  report.finishedAt = new Date().toISOString();
  writeFileSync(reportPath, JSON.stringify(report, null, 2), { flag: 'wx' });
}
// Imported upstream housekeeping must not turn this finite probe into a daemon.
process.exit(process.exitCode ?? 0);
