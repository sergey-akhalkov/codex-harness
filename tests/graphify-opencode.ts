// Read-only compatibility check through the installed source kit's real validators.
// bun tests/graphify-opencode.ts ../opencode-kit/tools/windows/opencode-shared-tools.ts <manifest>
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const source = process.argv[2];
const manifestPath = process.argv[3];
if (!source || !manifestPath) throw new Error('Supply the existing OpenCode module and workstation manifest paths.');
const { validateGraphifyConfiguration, assertReusableGraphifyConfig } = await import(pathToFileURL(path.resolve(source)).href);
const manifest = JSON.parse(readFileSync(manifestPath, 'utf8').replace(/^\uFEFF/, ''));
const graphify = manifest.graphify.configuration;
const observed = validateGraphifyConfiguration({ python: graphify.python.path, graph: graphify.graph.path,
  port: graphify.endpoint.port }, path.dirname(path.resolve(manifestPath)));
assert.deepEqual(observed.module, graphify.module, 'OpenCode module identity');
assert.equal(observed.python.sha256, graphify.python.sha256, 'Adopted Python identity');
assert.equal(observed.graph.sha256, graphify.graph.sha256, 'Saved graph snapshot identity');
const config = await assertReusableGraphifyConfig(manifest.graphify.configEdit.path, graphify);
console.log(JSON.stringify({ openCodeModuleIdentity: true, version: observed.module.packageVersion,
  pythonAndGraphIdentityPreserved: true, existingConfiguration: config.kind }));
