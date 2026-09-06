// Process-local compatibility for bash-language-server 5.6.0 on Windows.
// The upstream sourcing helper joins file URI strings as native paths and
// returns `file://C:\...`, missing its own canonical analyzed-document keys.
// Preserve its source-expression logic; adapt only that path/URI boundary.
// The shared installation and project files are never modified.
'use strict';
const path = require('node:path');
const {fileURLToPath, pathToFileURL} = require('node:url');
const entry = path.resolve(process.argv[1]);
const packageInfo = require(path.join(path.dirname(entry), '..', 'package.json'));
if (packageInfo.name !== 'bash-language-server' || packageInfo.version !== '5.6.0') {
  throw new Error('Windows Bash source URI compatibility requires reviewed bash-language-server 5.6.0');
}
const sourcing = require(path.join(path.dirname(entry), 'util', 'sourcing.js'));
const original = sourcing.getSourceCommands;
sourcing.getSourceCommands = function (parameters) {
  const fileUri = parameters.fileUri.startsWith('file:')
    ? fileURLToPath(parameters.fileUri) : parameters.fileUri;
  return original({...parameters, fileUri}).map(command => ({
    ...command,
    uri: command.uri && command.uri.startsWith('file://')
      ? pathToFileURL(command.uri.slice('file://'.length)).href : command.uri,
  }));
};
