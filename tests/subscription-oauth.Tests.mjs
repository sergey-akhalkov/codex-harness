// Bounded investigation of the pinned upstream OAuth callback implementation.
// No Bun, network, provider credentials, or live config. Run with Node >= 24:
// node --max-old-space-size=128 tests/subscription-oauth.Tests.mjs <package-root>
import assert from 'node:assert/strict';
import { readFileSync, mkdtempSync, rmdirSync, unlinkSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { stripTypeScriptTypes } from 'node:module';
import { createInterface } from 'node:readline';
import { PassThrough } from 'node:stream';
import { browserLoginPage, loginBrowserOnly } from '../tools/opencodex-login.mjs';

const packageRoot = process.argv[2];
assert.ok(packageRoot, 'Supply the installed OpenCodex package root.');
const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
assert.equal(metadata.version, '2.44.0', 'Reassess the reproduction for a different upstream version.');
let source = readFileSync(join(packageRoot, 'src/oauth/callback-server.ts'), 'utf8');
assert.equal(source.split('while (true) {').length, 2);
source = source.replace('import { isAddrInUse } from "../server/ports";', 'const isAddrInUse = () => false;');
source = source.replace('while (true) {', 'while (true) { if (++globalThis.__oauthProbeIterations > 20000) throw new Error("PROBE_ITERATION_CAP");');
const compiled = stripTypeScriptTypes(source);
const { OAuthCallbackFlow } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

let listener;
let stopped = 0;
globalThis.Bun = { serve(options) { listener = options.fetch; return { port: 56121, stop() { stopped++; } }; } };
class FakeFlow extends OAuthCallbackFlow {
  constructor(controller, browserCallback = false) { super(controller, { preferredPort: 56121, callbackHostname: '127.0.0.1' }); this.browserCallback = browserCallback; }
  async generateAuthUrl(state, redirectUri) {
    if (this.browserCallback) setImmediate(() => listener(new Request(`${redirectUri}?code=fake-code&state=${state}`)));
    return { url: 'https://example.invalid/authorization' };
  }
  async exchangeToken(code) { assert.equal(code, 'fake-code'); return { fake: true }; }
}

const rl = createInterface({ input: new PassThrough(), output: new PassThrough() });
rl.close();
let promptError;
let timerFired = false;
setImmediate(() => { timerFired = true; });
globalThis.__oauthProbeIterations = 0;
const heapBefore = process.memoryUsage().heapUsed;
await assert.rejects(new FakeFlow({
  onManualCodeInput: () => new Promise(resolve => rl.question('unused', resolve)).catch(error => { promptError = error.code; throw error; }),
}).login(), /PROBE_ITERATION_CAP/);
const heapGrowthBytes = process.memoryUsage().heapUsed - heapBefore;
assert.equal(promptError, 'ERR_USE_AFTER_CLOSE');
assert.equal(globalThis.__oauthProbeIterations, 20001);
assert.equal(timerFired, false, 'The upstream microtask loop starves scheduled I/O.');
assert.equal(stopped, 1);

globalThis.__oauthProbeIterations = 0;
assert.deepEqual(await new FakeFlow({}, true).login(), { fake: true });
assert.equal(globalThis.__oauthProbeIterations, 0, 'Browser-only mode must not enter the manual-input loop.');
assert.equal(timerFired, true);
assert.equal(stopped, 2);
const fixture = mkdtempSync(join(tmpdir(), 'harness-oauth-'));
const fakeAuthUrl = 'https://auth.x.ai/authorize?state=fake&redirect_uri=http%3A%2F%2F127.0.0.1%3A56121%2Fcallback';
assert.match(browserLoginPage(fakeAuthUrl), /action="http:\/\/127.0.0.1:56121\/callback"/);
assert.match(browserLoginPage(fakeAuthUrl), /type="hidden" name="state" value="fake"/);
try {
  const authorizationPath = join(fixture, 'authorization.json');
  const events = [];
  const signal = new AbortController().signal;
  const runLogin = async (provider, controller, options) => {
    assert.equal(provider, 'xai');
    assert.equal(controller.signal, signal);
    assert.equal(controller.onManualCodeInput, undefined);
    assert.deepEqual(options, { forceLogin: true });
    controller.onAuth({ url: fakeAuthUrl });
    assert.equal(JSON.parse(readFileSync(authorizationPath, 'utf8')).authorizationUrl, fakeAuthUrl);
    assert.equal(existsSync(authorizationPath + '.html'), true);
    return { accessToken: 'fixture-secret-must-not-be-returned' };
  };
  assert.equal(await loginBrowserOnly({ runLogin, provider: 'xai', authorizationPath, signal, progress: event => events.push(event) }), undefined);
  assert.deepEqual(events, ['browser_login_ready', 'login_saved']);
  assert.equal(existsSync(authorizationPath), false);
  assert.equal(existsSync(authorizationPath + '.html'), false);
  await assert.rejects(loginBrowserOnly({ provider: 'xai', authorizationPath, signal, runLogin: async (_, controller) => {
    controller.onAuth({ url: fakeAuthUrl });
    throw new Error('fixture failure');
  } }), /fixture failure/);
  assert.equal(existsSync(authorizationPath), false);
  assert.equal(existsSync(authorizationPath + '.html'), false);
  await assert.rejects(loginBrowserOnly({ provider: 'xai', authorizationPath, signal, runLogin: async (_, controller) => controller.onAuth({ url: fakeAuthUrl.replace('auth.x.ai', 'example.invalid') }) }), /Unexpected xAI/);
  assert.equal(existsSync(authorizationPath), false);
} finally {
  const residual = join(fixture, 'authorization.json');
  if (existsSync(residual)) unlinkSync(residual);
  if (existsSync(residual + '.html')) unlinkSync(residual + '.html');
  rmdirSync(fixture);
}
console.log(JSON.stringify({ upstreamVersion: metadata.version, reproduction: 'closed readline -> 20000 immediate retries, event-loop starvation', boundedIterations: 20000, heapGrowthBytes, browserOnlyCallback: 'passed', networkUsed: false, bunLaunched: false }));
