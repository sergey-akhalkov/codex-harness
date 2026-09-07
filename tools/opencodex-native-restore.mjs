// Run with the pinned package's Bun through opencodex-process.ps1 only.
// Do not use `ocx restore`: that changes reusable desired state and other clients.
import { readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [packageRoot] = process.argv.slice(2);
if (!packageRoot || !process.env.CODEX_HOME || !process.env.OPENCODEX_HOME) throw new Error('Explicit package and home paths required.');
const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
if (metadata.name !== '@bitkyc08/opencodex' || metadata.version !== '2.44.0') throw new Error('Native restore requires the audited package version.');
const { restoreNativeCodex } = await import(pathToFileURL(resolve(packageRoot, 'src/codex/inject.ts')).href);
const restored = restoreNativeCodex({ skipHistory: true });
console.log(JSON.stringify(restored));
process.exitCode = restored.success ? 0 : 1;
