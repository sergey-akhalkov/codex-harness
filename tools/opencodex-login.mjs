// Run with the pinned package's Bun inside tools/opencodex-process.ps1.
// The supervising host opens the authorization URL outside the process job.
import { readFileSync, writeFileSync, unlinkSync, existsSync } from 'node:fs';
import { isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export function browserLoginPage(url) {
  const parsed = new URL(url);
  const redirect = new URL(parsed.searchParams.get('redirect_uri'));
  const state = parsed.searchParams.get('state');
  if (parsed.protocol !== 'https:' || parsed.hostname !== 'auth.x.ai' || redirect.href !== 'http://127.0.0.1:56121/callback' || !state) throw new Error('Unexpected xAI authorization destination');
  const escape = value => value.replace(/[&<>"']/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character]);
  return `<!doctype html><html lang="ru"><meta charset="utf-8"><title>Вход в Grok</title>
<style>body{font:18px system-ui;max-width:640px;margin:70px auto;padding:24px;color:#222;background:#fafafa}a,button{display:inline-block;background:#222;color:white;padding:12px 18px;border:0;border-radius:8px;text-decoration:none;font:inherit}input[type=password]{display:block;box-sizing:border-box;width:100%;padding:12px;margin:12px 0;font:inherit}p{line-height:1.5}</style>
<h1>Подключить подписку Grok</h1><p>1. Войдите в xAI и подтвердите доступ. Страница откроется в новой вкладке.</p>
<a href="${escape(url)}" target="_blank" rel="noreferrer">Открыть вход xAI</a>
<p>2. Если xAI покажет код, скопируйте его и вставьте сюда. Если появится «Login complete», этот шаг уже выполнен.</p>
<form action="${escape(redirect.href)}" method="get" autocomplete="off"><input type="hidden" name="state" value="${escape(state)}"><label for="code">Одноразовый код</label><input id="code" name="code" type="password" required autocomplete="off"><button type="submit">Завершить вход</button></form>
<p>Окно входа действует около пяти минут. Код отправляется локальному процессу авторизации на этом ПК.</p></html>`;
}

export async function loginBrowserOnly({ runLogin, provider, authorizationPath, signal, progress = () => {} }) {
  let created = false;
  let pageCreated = false;
  const pagePath = authorizationPath + '.html';
  try {
    await runLogin(provider, {
      signal,
      onAuth: ({ url }) => {
        const page = browserLoginPage(url);
        writeFileSync(authorizationPath, JSON.stringify({ provider, authorizationUrl: url, pagePath }), { flag: 'wx', mode: 0o600 });
        created = true;
        writeFileSync(pagePath, page, { flag: 'wx', mode: 0o600 });
        pageCreated = true;
        progress('browser_login_ready');
      },
      onProgress: message => {
        if (message === 'Waiting for browser authentication...') progress('awaiting_browser_callback');
        if (message === 'Exchanging authorization code for tokens...') progress('exchanging_code');
      },
      // Deliberately no onManualCodeInput: upstream 2.44.0 spins on closed stdin.
    }, { forceLogin: true });
    progress('login_saved');
  } finally {
    if (created && existsSync(authorizationPath)) unlinkSync(authorizationPath);
    if (pageCreated && existsSync(pagePath)) unlinkSync(pagePath);
  }
}

async function main() {
  const [packageRoot, provider, authorizationPath] = process.argv.slice(2);
  if (!packageRoot || !isAbsolute(packageRoot) || provider !== 'xai' || !authorizationPath || !isAbsolute(authorizationPath)) {
    throw new Error('Usage: opencodex-login.mjs <absolute-package-root> xai <new-host-authorization-json>');
  }
  const metadata = JSON.parse(readFileSync(join(packageRoot, 'package.json'), 'utf8'));
  if (metadata.name !== '@bitkyc08/opencodex' || metadata.version !== '2.44.0') throw new Error('Unsupported OpenCodex package; reassess the login contract');
  if (!process.env.OPENCODEX_HOME || !process.env.CODEX_HOME) throw new Error('Explicit runtime homes are required');
  const { validateConfigCandidate } = await import(pathToFileURL(join(packageRoot, 'src/config.ts')).href);
  const validation = validateConfigCandidate(JSON.parse(readFileSync(join(process.env.OPENCODEX_HOME, 'config.json'), 'utf8')));
  if (!validation.ok) throw new Error('Invalid isolated OpenCodex configuration');
  const { runLogin } = await import(pathToFileURL(join(packageRoot, 'src/oauth/index.ts')).href);
  await loginBrowserOnly({ runLogin, provider, authorizationPath, signal: AbortSignal.timeout(330_000), progress: event => console.log(event) });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    // Raw OAuth errors can contain a callback URL or provider response body.
    console.error(`Browser login failed (${error?.name || 'Error'}); authentication was not confirmed.`);
    process.exitCode = 1;
  });
}
