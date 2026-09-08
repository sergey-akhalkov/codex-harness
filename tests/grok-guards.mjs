// Exercise the pinned runtime's owning mechanisms without models or services.
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import {join, isAbsolute} from 'node:path';
import {readFileSync} from 'node:fs';

const root = process.argv[2];
assert.ok(root && isAbsolute(root), 'Supply the installed pinned package root');
assert.equal(JSON.parse(readFileSync(join(root, 'package.json'))).version, '2.44.0');
const {guardEmptyCompletionEventStream, emptyCompletionRetryEnabled} = await import(pathToFileURL(join(root, 'src/server/responses/empty-completion-guard.ts')));
const {guardTerminalEventStream, analyzeTerminalTurn} = await import(pathToFileURL(join(root, 'src/server/responses/terminal-guard.ts')));
const {parseRequest} = await import(pathToFileURL(join(root, 'src/responses/parser.ts')));
async function* stream(events) { yield* events; }
async function collect(events) { const out=[]; for await (const event of events) out.push(event); return out; }
const empty = [{type:'thinking_delta',thinking:'synthetic'}, {type:'done'}];
const executed = [{type:'tool_call_start',id:'a',name:'wait'}, {type:'tool_call_delta',id:'a',arguments:'{"cell_id":"1"}'}, {type:'tool_call_end',id:'a'}, {type:'done'}];
assert.equal(emptyCompletionRetryEnabled({emptyCompletionRetry:true}, {}), true);
assert.equal(emptyCompletionRetryEnabled({emptyCompletionRetry:true}, {OCX_EMPTY_COMPLETION_RETRY:'0'}), false);
let retries=0;
const recovered = await collect(guardEmptyCompletionEventStream({firstEvents:stream(empty),continuation:()=>{retries++;return stream(executed);}}));
assert.equal(retries,1); assert.equal(recovered.at(-1).type,'done');
assert.equal(recovered.filter(e=>e.type==='tool_call_start').length,1);
retries=0;
const failed = await collect(guardEmptyCompletionEventStream({firstEvents:stream(empty),continuation:()=>{retries++;return stream(empty);}}));
assert.equal(retries,1);assert.equal(failed.at(-1).code,'empty_completion_retry_failed');
assert.equal(failed.at(-1).type,'error');
retries=0;
await collect(guardEmptyCompletionEventStream({firstEvents:stream(executed),continuation:()=>{retries++;return stream(executed);}}));
assert.equal(retries,0,'Already exposed tools must not be replayed');
const parsed = parseRequest({model:'grok-4.6',input:[{role:'user',content:'Run the delayed command, retrieve its result, then write the output file.'}],tools:[{type:'function',name:'wait',parameters:{type:'object',properties:{cell_id:{type:'string'}},required:['cell_id']}}]});
const statusOnly=[{type:'text_delta',text:"The command is still running; I'll wait for the delayed output."},{type:'done'}];
assert.equal(analyzeTerminalTurn(parsed,statusOnly).decision,'continue');
retries=0;
const continued = await collect(guardTerminalEventStream({parsed,adapterName:'openai-chat',firstEvents:stream(statusOnly),continuation:()=>{retries++;return stream(executed);},maxAutoContinuations:1}));
assert.equal(retries,1);assert.equal(continued.filter(e=>e.type==='tool_call_start').length,1);
retries=0;
await collect(guardTerminalEventStream({parsed,adapterName:'openai-chat',firstEvents:stream(statusOnly),continuation:()=>{retries++;return stream(statusOnly);},maxAutoContinuations:1}));
assert.equal(retries,1,'Status correction must remain bounded');
assert.equal(analyzeTerminalTurn(parsed,[{type:'text_delta',text:'Please provide the missing file?'},{type:'done'}]).decision,'pass');
console.log('PASS: pinned empty/status recovery, stop after repeat failure, no replay of exposed tools, clarification preserved');
