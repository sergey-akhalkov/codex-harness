// Protocol fixture only: proves Codex transport and hook delivery, not LSP analysis.
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { randomUUID } from 'node:crypto';
import { createInterface } from 'node:readline';

const [workspaceArg, tracePath, markerPath] = process.argv.slice(2);
const workspace = resolve(workspaceArg);
const trace = (record) => appendFileSync(tracePath, JSON.stringify(record) + '\n');
const tools = [
  { name: 'identity', description: 'Return the native integration fixture source marker.', inputSchema: { type: 'object', properties: {}, additionalProperties: false } },
  { name: 'write_note', description: 'Write the bounded native integration fixture note and return its original-result marker.', inputSchema: { type: 'object', properties: { text: { type: 'string' } }, required: ['text'], additionalProperties: false } },
  { name: 'diagnostics_after_tool', description: 'Reserved automatic hook protocol fixture. Do not call manually.', inputSchema: { type: 'object', additionalProperties: true } },
];

function resultFor(message) {
  if (message.method === 'initialize') return { protocolVersion: '2024-11-05', capabilities: { tools: {} }, serverInfo: { name: 'native-contract-fixture', version: '1' } };
  if (message.method === 'ping') return {};
  if (message.method === 'tools/list') return { tools };
  if (message.method === 'resources/list') return { resources: [] };
  if (message.method === 'resources/templates/list') return { resourceTemplates: [] };
  if (message.method !== 'tools/call') throw new Error('Unsupported fixture method: ' + message.method);
  const { name, arguments: input = {} } = message.params;
  trace({ method: message.method, name, input });
  if (name === 'identity') return { content: [{ type: 'text', text: readFileSync(markerPath, 'utf8').trim() }] };
  if (name === 'write_note') {
    const target = resolve(workspace, 'native-note.txt');
    if (dirname(target) !== workspace) throw new Error('Fixture write escaped workspace.');
    writeFileSync(target, input.text, 'utf8');
    return { content: [{ type: 'text', text: 'NATIVE_ORIGINAL_TOOL_RESULT: note written; ' + readFileSync(markerPath, 'utf8').trim() }] };
  }
  if (name === 'diagnostics_after_tool') {
    const nonce = 'NATIVE_HOOK_' + randomUUID().replaceAll('-', '');
    const output = { hookSpecificOutput: { hookEventName: 'PostToolUse', additionalContext: 'Automatic native contract check completed. Report this verification marker in the final answer: ' + nonce } };
    trace({ hookNonce: nonce, output });
    return { content: [{ type: 'text', text: JSON.stringify(output) }] };
  }
  throw new Error('Unsupported fixture tool: ' + name);
}

for await (const line of createInterface({ input: process.stdin, crlfDelay: Infinity })) {
  let message;
  try {
    message = JSON.parse(line);
    if (!Object.hasOwn(message, 'id')) continue;
    const result = resultFor(message);
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: message.id, result }) + '\n');
  } catch (error) {
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: message?.id ?? null, error: { code: -32603, message: String(error.message) } }) + '\n');
  }
}
