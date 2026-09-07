// Run through the adjacent opt-in PowerShell supervisor. All payloads are synthetic.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { syncBuiltinESMExports } from 'node:module';

const parametersPath = process.argv[2];
if (!parametersPath || !path.isAbsolute(parametersPath)) throw new Error('An absolute fixture parameters file is required');
const parameters = JSON.parse(fs.readFileSync(parametersPath, 'utf8'));
const fixture = path.resolve(parameters.fixture);
if (!/^codex-subscription-memory-[a-f0-9]{32}$/.test(path.basename(fixture)) || path.dirname(parametersPath) !== fixture) throw new Error('Invalid fixture boundary');
const proxyMode = process.argv[3] === '--proxy';
let runName = proxyMode ? process.argv[4] : 'cold';
if (!['cold', 'warm'].includes(runName)) throw new Error('Invalid proxy run name');
const here = fileURLToPath(import.meta.url);
const write = (name, value) => fs.writeFileSync(path.join(fixture, name), JSON.stringify(value, null, 2));
const append = (name, value) => fs.appendFileSync(path.join(fixture, name), JSON.stringify(value) + '\n');
const sleep = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds));
const syntheticAdmin = 'isolated-synthetic-memory-admin';
const user = path.join(fixture, 'user');
const codex = path.join(fixture, 'codex');
const opencodex = path.join(user, '.opencodex');
for (const key of Object.keys(process.env)) {
  if (/(?:TOKEN|SECRET|PASSWORD|API_KEY|BASE_URL|PROXY)/i.test(key) || /^(?:CODEX|OPENCODE|GROK|CLAUDE|XAI|OCX)_/.test(key)) delete process.env[key];
}
Object.assign(process.env, {
  USERPROFILE: user, HOME: user, HOMEDRIVE: path.parse(user).root.replace(/[\\/]$/, ''), HOMEPATH: user.slice(2),
  APPDATA: path.join(user, 'AppData/Roaming'), LOCALAPPDATA: path.join(user, 'AppData/Local'),
  XDG_CONFIG_HOME: path.join(user, '.config'), XDG_DATA_HOME: path.join(user, '.local/share'),
  CODEX_HOME: codex, CODEX_SQLITE_HOME: codex, OPENCODEX_HOME: opencodex, CLAUDE_CONFIG_DIR: path.join(user, '.claude'),
  OCX_TEST_HOME_GUARD: '1', OCX_REAL_HOME: parameters.realUser, OPENCODEX_ADMIN_AUTH_TOKEN: syntheticAdmin,
});
for (const directory of [user, codex, opencodex, process.env.XDG_CONFIG_HOME, process.env.XDG_DATA_HOME]) fs.mkdirSync(directory, { recursive: true });
if (path.resolve(os.homedir()).toLowerCase() !== user.toLowerCase()) throw new Error('Homedir escaped isolated fixture');
for (const location of [path.join(codex, 'auth.json'), path.join(opencodex, 'auth.json'), path.join(user, '.local/share/opencode/auth.json')]) {
  if (fs.existsSync(location)) throw new Error('Provider credentials unexpectedly present in synthetic fixture');
}
const allowedOrigins = new Set();
const rejectNetwork = kind => {
  append('network.jsonl', { time: new Date().toISOString(), pid: process.pid, kind, blocked: true });
  throw new Error('Synthetic fixture blocked an unexpected network request');
};
const originalFetch = globalThis.fetch;
globalThis.fetch = (input, init) => {
  const url = new URL(typeof input === 'string' || input instanceof URL ? input : input.url);
  if (!allowedOrigins.has(url.origin)) return Promise.reject(new Error('Unexpected origin')).catch(() => rejectNetwork('fetch'));
  return originalFetch(input, { ...init, redirect: 'error' });
};
// No alternate network transport is needed by this Responses/loopback fixture.
for (const name of ['node:http', 'node:https']) {
  const module = (await import(name)).default;
  module.request = () => rejectNetwork(name);
  module.get = () => rejectNetwork(name);
}
const net = (await import('node:net')).default;
net.Socket.prototype.connect = () => rejectNetwork('node:net');
const tls = (await import('node:tls')).default;
tls.connect = () => rejectNetwork('node:tls');
Bun.connect = () => rejectNetwork('Bun.connect');
syncBuiltinESMExports();

if (proxyMode) {
  const settings = JSON.parse(fs.readFileSync(path.join(fixture, 'proxy-settings.json'), 'utf8'));
  allowedOrigins.add(settings.mockOrigin);
  const { validateConfigCandidate } = await import(pathToFileURL(path.join(parameters.package, 'src/config.ts')).href);
  const candidate = JSON.parse(fs.readFileSync(path.join(opencodex, 'config.json'), 'utf8'));
  if (!validateConfigCandidate(candidate).ok) throw new Error('Synthetic config rejected by pinned schema before server startup');
  const { startServer } = await import(pathToFileURL(path.join(parameters.package, 'src/server/index.ts')).href);
  const server = startServer(0);
  const origin = `http://127.0.0.1:${server.port}`;
  allowedOrigins.add(origin);
  write(`proxy-${runName}-listener.json`, { origin, pid: process.pid, bunVersion: Bun.version, startedAt: new Date().toISOString() });
  const timer = setInterval(() => {
    append('bun-memory.jsonl', { time: new Date().toISOString(), pid: process.pid, ...process.memoryUsage() });
  }, 100);
  try {
    const deadline = Date.now() + 140_000;
    while (!fs.existsSync(path.join(fixture, `proxy-${runName}.stop`)) && Date.now() < deadline) await sleep(100);
  } finally {
    clearInterval(timer);
    await server.stop(true);
  }
  process.exit(0);
}

const report = {
  status: 'running', startedAt: new Date().toISOString(), fixture, requests: [], phases: [], proxyRuns: [],
  limits: { proxyMemoryMiB: 768, controllerMemoryMiB: 2048, outerSeconds: 180, proxySeconds: 150 },
  limitations: [
    'Synthetic ASCII byte workload; no tokenizer or token-count equivalence is asserted.',
    'Real HTTP Responses passthrough and forced continuation storage; local auth, no canonical ChatGPT authentication or TLS.',
    'Direct startServer entry point skips CLI catalog prewarm and startup client injection; providers use static catalogs.',
    'Snapshot is produced by synthetic requests and reloaded in a second isolated proxy; real tool/history shape is not reproduced.',
    'Proxy memory excludes mock/generator; Windows Job peak includes proxy descendants and Windows samples have one-second cadence.',
  ],
};
let proxyRunner;
let mock;
let origin;
let sequence = 0;
let unexpectedMockRequests = 0;
const numeric = value => {
  if (typeof value === 'number' || typeof value === 'boolean' || value === null) return value;
  if (Array.isArray(value)) return value.map(numeric);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).filter(([, child]) => typeof child !== 'string').map(([key, child]) => [key, numeric(child)]));
  return undefined;
};
const sample = async phase => {
  const response = await fetch(`${origin}/api/system/memory`, { headers: { 'x-opencodex-api-key': syntheticAdmin }, signal: AbortSignal.timeout(10_000) });
  if (!response.ok) throw new Error(`Memory endpoint status ${response.status}`);
  const data = await response.json();
  const result = { run: runName, phase, time: new Date().toISOString(), ...numeric(data), streamMode: data.streamMode, bunVersion: data.bunVersion };
  report.phases.push(result);
  write('report.json', report);
};
const stopProxy = async () => {
  if (!proxyRunner) return;
  fs.writeFileSync(path.join(fixture, `proxy-${runName}.stop`), 'stop');
  const stopped = await Promise.race([proxyRunner.exited.then(() => true), sleep(12_000).then(() => false)]);
  if (!stopped) {
    proxyRunner.kill();
    await proxyRunner.exited;
    report.cleanupFailure = 'Proxy job owner required termination; its job closes all descendants';
  }
  proxyRunner = null;
  const resultPath = path.join(fixture, `proxy-${runName}.result.json`);
  const result = fs.existsSync(resultPath) ? JSON.parse(fs.readFileSync(resultPath, 'utf8')) : null;
  report.proxyRuns.push({ run: runName, result });
  if (!stopped || !result || result.ExitCode !== 0) throw new Error(`Proxy ${runName} did not exit successfully (${result?.Status ?? 'no result'})`);
};
const startProxy = async () => {
  const prefix = `proxy-${runName}`;
  write(`${prefix}.request.json`, {
    executable: path.join(parameters.package, 'node_modules/bun/bin/bun.exe'), arguments: ['--no-env-file', here, parametersPath, '--proxy', runName], workingDirectory: fixture,
    stdoutPath: path.join(fixture, `${prefix}.stdout`), stderrPath: path.join(fixture, `${prefix}.stderr`), startedPath: path.join(fixture, `${prefix}-started.json`),
    environment: {}, memoryLimitMiB: 768, timeoutSeconds: 150,
  });
  proxyRunner = Bun.spawn([parameters.powershell, '-NoLogo', '-NoProfile', '-File', path.join(parameters.repository, 'tests/subscription-memory.Tests.ps1'), '-ProxyRequest', path.join(fixture, `${prefix}.request.json`)], { cwd: fixture, windowsHide: true, stdout: Bun.file(path.join(fixture, `${prefix}-runner.stdout`)), stderr: Bun.file(path.join(fixture, `${prefix}-runner.stderr`)) });
  const waitStart = Date.now();
  while (!fs.existsSync(path.join(fixture, `${prefix}-listener.json`))) {
    if (proxyRunner.exitCode !== null) throw new Error('Proxy job exited before opening its listener');
    if (Date.now() - waitStart > 50_000) throw new Error('Proxy startup exceeded 50 seconds');
    await sleep(100);
  }
  const listener = JSON.parse(fs.readFileSync(path.join(fixture, `${prefix}-listener.json`), 'utf8'));
  origin = listener.origin;
  allowedOrigins.add(origin);
  await sample('baseline');
};
try {
  mock = Bun.serve({ hostname: '127.0.0.1', port: 0, maxRequestBodySize: 8 * 1024 * 1024, async fetch(request) {
    if (request.method !== 'POST' || new URL(request.url).pathname !== '/v1/responses') {
      unexpectedMockRequests++;
      return new Response('Unexpected synthetic upstream route', { status: 404 });
    }
    if (request.headers.has('authorization') || request.headers.has('chatgpt-account-id')) throw new Error('Unexpected provider credential on mock request');
    const bytes = await request.arrayBuffer();
    const body = JSON.parse(new TextDecoder().decode(bytes));
    const index = ++sequence;
    append('mock-metadata.jsonl', { time: new Date().toISOString(), index, bytes: bytes.byteLength, model: body.model, stream: body.stream });
    if (body.model !== 'gpt-6-astra' || body.stream !== true || !Array.isArray(body.input)) return new Response('Unexpected synthetic input shape', { status: 400 });
    const text = `SYNTHETIC_${index}_`.repeat(100);
    const item = { id: `msg_synthetic_${index}`, type: 'message', role: 'assistant', status: 'completed', content: [{ type: 'output_text', text, annotations: [] }] };
    const completed = { id: `resp_synthetic_${index}`, object: 'response', created_at: Math.floor(Date.now() / 1000), status: 'completed', model: 'gpt-6-astra', output: [item], usage: { input_tokens: Math.ceil(bytes.byteLength / 4), output_tokens: 350, total_tokens: Math.ceil(bytes.byteLength / 4) + 350 } };
    const encode = (type, data) => new TextEncoder().encode(`event: ${type}\ndata: ${JSON.stringify({ type, ...data })}\n\n`);
    const stream = new ReadableStream({ async start(controller) {
      controller.enqueue(encode('response.created', { response: { ...completed, status: 'in_progress', output: [] } }));
      controller.enqueue(encode('response.output_item.added', { output_index: 0, item: { ...item, status: 'in_progress', content: [] } }));
      controller.enqueue(encode('response.content_part.added', { output_index: 0, item_id: item.id, content_index: 0, part: { type: 'output_text', text: '', annotations: [] } }));
      for (let offset = 0; offset < text.length; offset += 64) {
        controller.enqueue(encode('response.output_text.delta', { output_index: 0, item_id: item.id, content_index: 0, delta: text.slice(offset, offset + 64) }));
        await sleep(20);
      }
      controller.enqueue(encode('response.output_text.done', { output_index: 0, item_id: item.id, content_index: 0, text }));
      controller.enqueue(encode('response.content_part.done', { output_index: 0, item_id: item.id, content_index: 0, part: item.content[0] }));
      controller.enqueue(encode('response.output_item.done', { output_index: 0, item: item }));
      controller.enqueue(encode('response.completed', { response: completed }));
      controller.enqueue(new TextEncoder().encode('data: [DONE]\n\n'));
      controller.close();
    } });
    return new Response(stream, { headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' } });
  } });
  const mockOrigin = `http://127.0.0.1:${mock.port}`;
  const provider = { adapter: 'openai-responses', baseUrl: `${mockOrigin}/v1`, allowPrivateNetwork: true, authMode: 'local', liveModels: false, models: ['gpt-6-astra'], defaultModel: 'gpt-6-astra' };
  write('proxy-settings.json', { mockOrigin });
  fs.writeFileSync(path.join(opencodex, 'config.json'), JSON.stringify({
    port: 10101, hostname: '127.0.0.1', providers: { openai: { ...provider, disabled: true }, fixture: provider }, defaultProvider: 'fixture',
    clientIntegrations: { codex: false, grok: false, 'claude-desktop': false }, codexAutoStart: false, codexShimAutoRestore: false, syncResumeHistory: false,
    tokenGuardian: { enabled: false }, webSearchSidecar: { enabled: false }, visionSidecar: { enabled: false },
    multiAgentMode: 'v1', websockets: false, appOwnedMemoryBudgetMb: 256, managementUsageMaxReadBytes: 67108864,
  }));
  await startProxy();
  await sleep(3000);
  await sample('idle-baseline');
  const issue = async (index, textBytes, phase) => {
    const prefix = `synthetic-${index}-`;
    const text = prefix.repeat(Math.ceil(textBytes / prefix.length)).slice(0, textBytes);
    const body = JSON.stringify({ model: 'fixture/gpt-6-astra', input: [{ role: 'user', content: [{ type: 'input_text', text }] }], stream: true, store: false });
    const started = Date.now();
    const response = await fetch(`${origin}/v1/responses`, { method: 'POST', headers: { 'Content-Type': 'application/json', 'thread-id': `synthetic-thread-${index}` }, body, signal: AbortSignal.timeout(20_000) });
    const result = await response.text();
    report.requests.push({ run: runName, index, phase, time: new Date(started).toISOString(), textBytes, wireBytes: Buffer.byteLength(body), responseBytes: Buffer.byteLength(result), status: response.status, durationMs: Date.now() - started, completed: result.includes('response.completed') });
    write('report.json', report);
    if (!response.ok || !result.includes('response.completed')) throw new Error(`Synthetic request ${index} failed (status ${response.status})`);
  };
  for (let index = 0; index < 7; index++) {
    await issue(index, 1000000, 'serial');
    await sample(`serial-${index}`);
  }
  for (let index = 7; index < 26; index++) {
    await issue(index, 1000000, 'seed-persisted-state');
    if (index % 4 === 1) await sample(`seed-${index}`);
  }
  await sample('seed-complete');
  await sleep(35_000);
  await sample('seed-retained-35s');
  const snapshot = path.join(opencodex, 'responses-state.json');
  report.seedSnapshotBytes = fs.existsSync(snapshot) ? fs.statSync(snapshot).size : 0;
  if (report.seedSnapshotBytes < 23_000_000 || report.seedSnapshotBytes > 26_000_000) throw new Error(`Synthetic persisted snapshot did not reach expected size (${report.seedSnapshotBytes} bytes)`);
  await stopProxy();
  runName = 'warm';
  await startProxy();
  for (let index = 26; index < 33; index++) {
    await issue(index, 700000 + (index - 26) * 50000, 'serial');
    await sample(`serial-${index}`);
  }
  await sample('before-concurrent');
  await Promise.all([33, 34, 35].map(index => issue(index, 1000000, 'concurrent')));
  await sample('after-concurrent');
  for (let elapsed = 0; elapsed < 35; elapsed += 5) {
    await sleep(5000);
    await sample(`retained-${elapsed + 5}s`);
  }
  report.snapshotBytes = fs.existsSync(snapshot) ? fs.statSync(snapshot).size : 0;
  report.mockRequests = sequence;
  report.unexpectedMockRequests = unexpectedMockRequests;
  report.blockedNetworkRequests = fs.existsSync(path.join(fixture, 'network.jsonl')) ? fs.readFileSync(path.join(fixture, 'network.jsonl'), 'utf8').trim().split('\n').filter(Boolean).length : 0;
  if (sequence !== 36 || unexpectedMockRequests || report.blockedNetworkRequests) throw new Error('Synthetic upstream/network count did not match the bounded workload');
  report.status = 'passed';
} catch (error) {
  report.status = 'failed';
  report.failure = error instanceof Error ? error.message : 'Unknown synthetic probe failure';
} finally {
  try { await stopProxy(); } catch (error) { report.status = 'failed'; report.cleanupFailure = error.message; }
  if (mock) await mock.stop(true);
  report.completedAt = new Date().toISOString();
  write('report.json', report);
  console.log(JSON.stringify({ status: report.status, fixture, requests: report.requests.length, failure: report.failure, peakJobBytes: report.proxyRuns.map(run => run.result?.PeakJobMemoryBytes) }));
}
process.exit(report.status === 'passed' ? 0 : 1);
