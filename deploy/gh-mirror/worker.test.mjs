import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import worker from './worker.js';

const MIRROR = 'https://clash-verge-tui.wty-yy.top';
const REPOSITORY = 'wty-yy/clash-verge-tui';
const VERSION = 'v1.4.11';
const ASSET = 'clash-verge-tui-v1.4.11-linux-x86_64.tar.gz';
const originalFetch = globalThis.fetch;
const originalCaches = globalThis.caches;

function environment() {
  const store = new Map();
  const calls = [];
  const pending = [];
  globalThis.caches = {
    default: {
      async match(key) {
        return store.get(key.url);
      },
      async put(key, response) {
        store.set(key.url, response);
      },
    },
  };
  globalThis.fetch = async (target) => {
    const url = target.toString();
    calls.push(url);
    const body = `mirror fixture: ${url}`;
    return new Response(body, {
      status: 200,
      headers: {
        'content-type': 'application/octet-stream',
        'content-length': String(body.length),
        'content-disposition': `attachment; filename="${ASSET}"`,
        etag: '"fixture"',
      },
    });
  };
  return { calls, pending, ctx: { waitUntil(promise) { pending.push(promise); } } };
}

function request(path, init) {
  return new Request(`${MIRROR}${path}`, init);
}

test.afterEach(() => {
  globalThis.fetch = originalFetch;
  if (originalCaches === undefined) {
    delete globalThis.caches;
  } else {
    globalThis.caches = originalCaches;
  }
});

test('proxies the absolute URL form used by install.sh', async () => {
  const { calls, ctx } = environment();
  const target = `https://github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`;
  const response = await worker.fetch(request(`/${target}`), {}, ctx);
  assert.equal(response.status, 200);
  assert.deepEqual(calls, [target]);
  assert.equal(response.headers.get('cache-control'), 'public, max-age=31536000, immutable');
  assert.equal(response.headers.get('x-mirror-cache'), 'miss');
  assert.equal(response.headers.get('content-disposition'), `attachment; filename="${ASSET}"`);
  assert.equal(await response.text(), `mirror fixture: ${target}`);
});

test('accepts collapsed absolute paths and checksum assets', async () => {
  const { calls, ctx } = environment();
  const response = await worker.fetch(
    request(`/https:/github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}.sha256`),
    {},
    ctx
  );
  assert.equal(response.status, 200);
  assert.deepEqual(calls, [`https://github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}.sha256`]);
});

test('serves clean repository paths', async () => {
  const { calls, ctx } = environment();
  const response = await worker.fetch(request(`/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`), {}, ctx);
  assert.equal(response.status, 200);
  assert.deepEqual(calls, [`https://github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`]);
});

test('serves the installer from the mirror root', async () => {
  const { calls, ctx } = environment();
  const response = await worker.fetch(request('/install.sh'), {}, ctx);
  assert.equal(response.status, 200);
  assert.deepEqual(calls, [`https://github.com/${REPOSITORY}/releases/latest/download/install.sh`]);
  assert.equal(response.headers.get('cache-control'), 'public, max-age=300');
  assert.equal(await response.text(), `mirror fixture: https://github.com/${REPOSITORY}/releases/latest/download/install.sh`);
});

test('resolves the latest and version shortcuts', async () => {
  const latest = environment();
  const latestResponse = await worker.fetch(request(`/latest/${ASSET}`), {}, latest.ctx);
  assert.equal(latestResponse.status, 200);
  assert.deepEqual(latest.calls, [`https://github.com/${REPOSITORY}/releases/latest/download/${ASSET}`]);
  assert.equal(latestResponse.headers.get('cache-control'), 'public, max-age=300');

  const version = environment();
  await worker.fetch(request(`/${VERSION}/${ASSET}`), {}, version.ctx);
  assert.deepEqual(version.calls, [`https://github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`]);
});

test('reuses the edge cache for repeated downloads', async () => {
  const { calls, pending, ctx } = environment();
  const target = `https://github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`;
  const first = await worker.fetch(request(`/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`), {}, ctx);
  await Promise.all(pending);
  assert.equal(await first.text(), `mirror fixture: ${target}`);
  const second = await worker.fetch(request(`/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`), {}, ctx);
  assert.equal(second.headers.get('x-mirror-cache'), 'hit');
  assert.equal(await second.text(), `mirror fixture: ${target}`);
  assert.deepEqual(calls, [target]);
});

test('rejects repositories outside the configured origin', async () => {
  const { calls, ctx } = environment();
  const denied = await worker.fetch(request(`/https://github.com/example/other/releases/download/${VERSION}/${ASSET}`), {}, ctx);
  assert.equal(denied.status, 404);
  assert.match(await denied.text(), /Supported URLs/);
  const outside = await worker.fetch(request(`/https://example.com/${ASSET}`), {}, ctx);
  assert.equal(outside.status, 404);
  assert.deepEqual(calls, []);
});

test('rejects traversal and unsupported shapes', async () => {
  const { calls, ctx } = environment();
  for (const path of [
    `/https://github.com/${REPOSITORY}/releases/download/${VERSION}/..%2F..%2Fsecret`,
    `/${REPOSITORY}/releases/download/${VERSION}/`,
    `/${REPOSITORY}/releases/latest/download/unsupported/extra`,
  ]) {
    const response = await worker.fetch(request(path), {}, ctx);
    assert.equal(response.status, 404, path);
  }
  assert.deepEqual(calls, []);
});

test('answers health checks and usage pages', async () => {
  const { calls, ctx } = environment();
  const health = await worker.fetch(request('/health'), {}, ctx);
  assert.equal(health.status, 200);
  assert.equal(await health.text(), 'ok\n');
  const usage = await worker.fetch(request('/'), {}, ctx);
  assert.equal(usage.status, 200);
  const page = await usage.text();
  assert.match(page, /--source proxy/);
  assert.ok(page.includes(`${MIRROR}/install.sh | sh -s -- --source proxy`));
  assert.deepEqual(calls, []);
});

test('allows GET and HEAD only', async () => {
  const { calls, ctx } = environment();
  const response = await worker.fetch(request(`/${VERSION}/${ASSET}`, { method: 'POST' }), {}, ctx);
  assert.equal(response.status, 405);
  assert.equal(response.headers.get('allow'), 'GET, HEAD');
  assert.deepEqual(calls, []);
});

test('probes HEAD without caching', async () => {
  const { calls, pending, ctx } = environment();
  const response = await worker.fetch(request(`/${VERSION}/${ASSET}`, { method: 'HEAD' }), {}, ctx);
  assert.equal(response.status, 200);
  assert.equal(response.body, null);
  assert.deepEqual(calls, [`https://github.com/${REPOSITORY}/releases/download/${VERSION}/${ASSET}`]);
  assert.deepEqual(pending, []);
});

test('honours the MIRROR_DOMAIN and ORIGIN variables', async () => {
  const { calls, ctx } = environment();
  const env = { MIRROR_DOMAIN: 'gh.example.com', ORIGIN: 'https://mirror.example.com/example/repo.git' };
  const response = await worker.fetch(request(`/latest/${ASSET}`), env, ctx);
  assert.equal(response.headers.get('x-mirror'), 'gh.example.com');
  assert.deepEqual(calls, [`https://mirror.example.com/example/repo/releases/latest/download/${ASSET}`]);
  const usage = await worker.fetch(request('/'), env, ctx);
  const page = await usage.text();
  assert.match(page, /gh\.example\.com/);
  assert.match(page, /mirror\.example\.com\/example\/repo/);
});

test('serves pinned MetaCubeX GeoData, UI, and core assets', async () => {
  const geodata = environment();
  const metadb = await worker.fetch(request('/geodata/geoip.metadb'), {}, geodata.ctx);
  assert.equal(metadb.status, 200);
  assert.deepEqual(geodata.calls, [
    'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.metadb',
  ]);
  const ui = environment();
  const metacubexd = await worker.fetch(request('/ui/metacubexd.tar.gz'), {}, ui.ctx);
  assert.equal(metacubexd.status, 200);
  assert.equal(metacubexd.headers.get('cache-control'), 'public, max-age=300');
  assert.deepEqual(ui.calls, ['https://codeload.github.com/MetaCubeX/metacubexd/tar.gz/refs/heads/gh-pages']);
  const yacd = environment();
  await worker.fetch(request('/ui/yacd-meta.tar.gz'), {}, yacd.ctx);
  assert.deepEqual(yacd.calls, ['https://codeload.github.com/MetaCubeX/Yacd-meta/tar.gz/refs/heads/gh-pages']);
  const core = environment();
  const version = 'v1.19.29';
  const asset = 'mihomo-linux-amd64-v1.19.29.gz';
  const response = await worker.fetch(request(`/core/${version}/${asset}`), {}, core.ctx);
  assert.equal(response.status, 200);
  assert.deepEqual(core.calls, [`https://github.com/MetaCubeX/mihomo/releases/download/${version}/${asset}`]);
  assert.equal(response.headers.get('cache-control'), 'public, max-age=31536000, immutable');
});

test('rejects unpinned GeoData, UI, and core shapes', async () => {
  const { calls, ctx } = environment();
  for (const path of [
    '/geodata/GeoIP.dat.bak',
    '/geodata/../geoip.metadb',
    '/ui/unknown-panel.tar.gz',
    '/ui/metacubexd.zip',
    '/core/v1.19.29/mihomo-linux-amd64-v1.19.30.gz',
    '/core/v1.19.29/evil.tar.gz',
    '/core/1.19.29/mihomo-linux-amd64-v1.19.29.gz',
  ]) {
    const response = await worker.fetch(request(path), {}, ctx);
    assert.equal(response.status, 404, path);
  }
  assert.deepEqual(calls, []);
});

test('resolves the latest release tag without the GitHub API', async () => {
  const { pending, ctx } = environment();
  globalThis.fetch = async (target, init) => {
    assert.equal(target.toString(), `https://github.com/${REPOSITORY}/releases/latest`);
    assert.equal(init.redirect, 'follow');
    const response = new Response('html', { status: 200 });
    Object.defineProperty(response, 'url', {
      value: `https://github.com/${REPOSITORY}/releases/tag/${VERSION}`,
    });
    return response;
  };
  const first = await worker.fetch(request('/latest-version'), {}, ctx);
  assert.equal(first.status, 200);
  assert.equal(await first.text(), `${VERSION}\n`);
  await Promise.all(pending);
  const second = await worker.fetch(request('/latest-version'), {}, ctx);
  assert.equal(second.headers.get('x-mirror-cache'), 'hit');
  assert.equal(await second.text(), `${VERSION}\n`);
});

test('reports an unavailable latest release tag', async () => {
  const { ctx } = environment();
  globalThis.fetch = async () => new Response(null, { status: 404 });
  const response = await worker.fetch(request('/latest-version'), {}, ctx);
  assert.equal(response.status, 502);
});

test('rejects an invalid ORIGIN variable', async () => {
  const { calls, ctx } = environment();
  for (const origin of ['http://github.com/wty-yy/clash-verge-tui', 'https://github.com/only-owner', '']) {
    const response = await worker.fetch(request(`/latest/${ASSET}`), { ORIGIN: origin }, ctx);
    assert.equal(response.status, 404, origin);
  }
  assert.deepEqual(calls, []);
});

test('ships the default deployment configuration', () => {
  const configuration = JSON.parse(readFileSync(new URL('./wrangler.json', import.meta.url), 'utf8'));
  assert.equal(configuration.vars.MIRROR_DOMAIN, 'clash-verge-tui.wty-yy.top');
  assert.equal(configuration.vars.ORIGIN, 'https://github.com/wty-yy/clash-verge-tui');
  assert.equal(configuration.routes[0].custom_domain, true);
});
