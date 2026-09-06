// The official VS Code Markdown LSP delegates tokenization to its client.
// Resolve the already provisioned parser by an explicit host-owned path.
const fs = require('node:fs');
const MarkdownIt = require(process.argv[2]);
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
process.stdout.write(JSON.stringify(new MarkdownIt({html: true}).parse(input.text, {})));
