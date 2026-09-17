// Cloudflare Worker: transparent GitHub release mirror for clash-verge-tui.wty-yy.top.
//
// Configured with two variables:
//   MIRROR_DOMAIN  public mirror hostname, e.g. clash-verge-tui.wty-yy.top
//   ORIGIN         upstream repository, e.g. https://github.com/wty-yy/clash-verge-tui
//
// Accepted URL forms:
//   https://<MIRROR_DOMAIN>/https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>
//   https://<MIRROR_DOMAIN>/<owner>/<repo>/releases/download/<tag>/<asset>
//   https://<MIRROR_DOMAIN>/latest/<asset>
//   https://<MIRROR_DOMAIN>/v<major>.<minor>.<patch>/<asset>
//   https://<MIRROR_DOMAIN>/geodata/<geoip.dat|geosite.dat|geoip.metadb|country.mmdb|GeoLite2-ASN.mmdb>
//   https://<MIRROR_DOMAIN>/ui/<metacubexd|yacd-meta>.tar.gz
//   https://<MIRROR_DOMAIN>/core/v<version>/mihomo-linux-<amd64|arm64>-v<version>.gz
//   https://<MIRROR_DOMAIN>/latest-version
//
// Only release assets of the configured repository plus the pinned MetaCubeX
// GeoData, Web UI, and mihomo assets are proxied; files are neither stored nor
// rewritten, responses are cached at the Cloudflare edge.
const DEFAULT_DOMAIN = 'clash-verge-tui.wty-yy.top';
const DEFAULT_ORIGIN = 'https://github.com/wty-yy/clash-verge-tui';
const PINNED_TTL = 31536000;
const LATEST_TTL = 300;
const REPOSITORY_PATH = /^\/[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;
const RELEASE_SUFFIX = /^(?:download\/v[0-9]+\.[0-9]+\.[0-9]+\/|latest\/download\/)[A-Za-z0-9._-]+$/;
const SHORT_PATH = /^\/(latest|v[0-9]+\.[0-9]+\.[0-9]+)\/([A-Za-z0-9._-]+)$/;
const META_RULES = 'https://github.com/MetaCubeX/meta-rules-dat';
const MIHOMO = 'https://github.com/MetaCubeX/mihomo';
const CODELOAD = 'https://codeload.github.com';
const GEODATA_PATH = /^\/geodata\/([A-Za-z0-9._-]+)$/;
const GEODATA_FILES = new Set(['geoip.dat', 'geosite.dat', 'geoip.metadb', 'country.mmdb', 'GeoLite2-ASN.mmdb']);
const UI_PATH = /^\/ui\/([a-z0-9-]+)\.tar\.gz$/;
const UI_PANELS = new Map([
  ['metacubexd', 'MetaCubeX/metacubexd'],
  ['yacd-meta', 'MetaCubeX/Yacd-meta'],
]);
const CORE_PATH = /^\/core\/v([0-9]+\.[0-9]+\.[0-9]+)\/([A-Za-z0-9._-]+)$/;
const CORE_ASSET = /^mihomo-linux-(?:amd64|arm64)-v[0-9]+\.[0-9]+\.[0-9]+\.gz$/;
const VERSION_TAG = /^v[0-9]+\.[0-9]+\.[0-9]+$/;

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return text('method not allowed\n', 405, { allow: 'GET, HEAD' });
    }
    if (url.pathname === '/health') {
      return text('ok\n', 200);
    }
    const configuration = { domain: mirrorDomain(env), origin: upstreamRepository(env) };
    if (configuration.origin === null) {
      return text(usage(configuration), url.pathname === '/' ? 200 : 404);
    }
    if (url.pathname === '/latest-version') {
      return latestVersion(configuration, ctx);
    }
    const target = resolvePinned(url.pathname) ?? resolveTarget(url.pathname, configuration.origin);
    if (target === null) {
      return text(usage(configuration), url.pathname === '/' ? 200 : 404);
    }
    return mirror(target, request, configuration, ctx);
  },
};

function mirrorDomain(env) {
  const value = (env.MIRROR_DOMAIN ?? DEFAULT_DOMAIN).trim();
  return value === '' ? DEFAULT_DOMAIN : value;
}

function upstreamRepository(env) {
  const candidate = parseUrl((env.ORIGIN ?? DEFAULT_ORIGIN).trim());
  if (candidate === null || candidate.protocol !== 'https:') {
    return null;
  }
  const path = candidate.pathname.replace(/\/+$/, '').replace(/\.git$/, '');
  if (!REPOSITORY_PATH.test(path)) {
    return null;
  }
  candidate.pathname = path;
  candidate.search = '';
  candidate.hash = '';
  return candidate;
}

function resolveTarget(pathname, origin) {
  let path;
  try {
    path = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const base = `${origin.origin}${origin.pathname}`;
  if (path === '/install.sh') {
    return new URL(`${base}/releases/latest/download/install.sh`);
  }
  if (path.startsWith('/https:/')) {
    return releaseTarget(parseUrl(path.replace(/^\/https:\/*/, 'https://')), origin);
  }
  const short = SHORT_PATH.exec(path);
  if (short !== null) {
    const download = short[1] === 'latest' ? 'latest/download' : `download/${short[1]}`;
    return new URL(`${base}/releases/${download}/${short[2]}`);
  }
  if (!path.startsWith('/') || path.startsWith('//')) {
    return null;
  }
  return releaseTarget(parseUrl(`${origin.origin}${path}`), origin);
}

/// Runtime assets pinned to MetaCubeX repositories: GeoData updates, the Web
/// UI archive, and the mihomo core used by source installations.
function resolvePinned(pathname) {
  let path;
  try {
    path = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const geodata = GEODATA_PATH.exec(path);
  if (geodata !== null && GEODATA_FILES.has(geodata[1])) {
    return new URL(`${META_RULES}/releases/download/latest/${geodata[1]}`);
  }
  const ui = UI_PATH.exec(path);
  if (ui !== null && UI_PANELS.has(ui[1])) {
    return new URL(`${CODELOAD}/${UI_PANELS.get(ui[1])}/tar.gz/refs/heads/gh-pages`);
  }
  const core = CORE_PATH.exec(path);
  if (core !== null && CORE_ASSET.test(core[2]) && core[2].includes(`v${core[1]}`)) {
    return new URL(`${MIHOMO}/releases/download/v${core[1]}/${core[2]}`);
  }
  return null;
}

/// Newest release tag of the configured repository, resolved from the
/// `releases/latest` redirect so clients avoid api.github.com.
async function latestVersion(configuration, ctx) {
  const target = new URL(`${configuration.origin.origin}${configuration.origin.pathname}/releases/latest`);
  const cache = caches.default;
  const cacheKey = new Request(target.toString(), { method: 'GET' });
  const cached = await cache.match(cacheKey);
  if (cached !== undefined) {
    return relay(cached, LATEST_TTL, 'hit', configuration.domain);
  }
  const upstream = await fetch(target, {
    redirect: 'follow',
    headers: {
      accept: 'text/html',
      'user-agent': 'clash-verge-tui-mirror (+https://github.com/wty-yy/clash-verge-tui)',
    },
  });
  await upstream.body?.cancel();
  const tag = (upstream.url ?? '').split('/').filter(Boolean).pop();
  if (!VERSION_TAG.test(tag ?? '')) {
    return text('latest version unavailable\n', 502);
  }
  const headers = new Headers({ 'content-type': 'text/plain; charset=utf-8' });
  const response = new Response(`${tag}\n`, {
    status: 200,
    headers: relayHeaders(headers, LATEST_TTL, 'miss', configuration.domain),
  });
  ctx.waitUntil(cache.put(cacheKey, response.clone()));
  return response;
}

function parseUrl(candidate) {
  try {
    return new URL(candidate);
  } catch {
    return null;
  }
}

function releaseTarget(candidate, origin) {
  if (candidate === null || candidate.origin !== origin.origin) {
    return null;
  }
  const prefix = `${origin.pathname}/releases/`;
  if (!candidate.pathname.startsWith(prefix)) {
    return null;
  }
  return RELEASE_SUFFIX.test(candidate.pathname.slice(prefix.length)) ? candidate : null;
}

/// Pinned release and core assets are immutable; `latest/` assets and branch
/// archives must stay short-lived so upstream refreshes are picked up.
function cacheMaxAge(pathname) {
  if (pathname.includes('/latest/') || pathname.includes('/refs/heads/')) {
    return LATEST_TTL;
  }
  return PINNED_TTL;
}

async function mirror(target, request, configuration, ctx) {
  const maxAge = cacheMaxAge(target.pathname);
  const cache = caches.default;
  const cacheKey = new Request(target.toString(), { method: 'GET' });
  if (request.method === 'GET') {
    const cached = await cache.match(cacheKey);
    if (cached !== undefined) {
      return relay(cached, maxAge, 'hit', configuration.domain);
    }
  }
  const upstream = await fetch(target, {
    method: 'GET',
    redirect: 'follow',
    headers: {
      accept: '*/*',
      'user-agent': 'clash-verge-tui-mirror (+https://github.com/wty-yy/clash-verge-tui)',
    },
  });
  if (request.method === 'HEAD') {
    return new Response(null, {
      status: upstream.status,
      headers: relayHeaders(upstream.headers, maxAge, 'miss', configuration.domain),
    });
  }
  if (upstream.ok && upstream.body !== null) {
    const stored = new Response(upstream.clone().body, {
      status: upstream.status,
      headers: relayHeaders(upstream.headers, maxAge, 'hit', configuration.domain),
    });
    ctx.waitUntil(cache.put(cacheKey, stored));
  }
  return relay(upstream, maxAge, 'miss', configuration.domain);
}

function relay(response, maxAge, cacheState, domain) {
  return new Response(response.body, { status: response.status, headers: relayHeaders(response.headers, maxAge, cacheState, domain) });
}

function relayHeaders(source, maxAge, cacheState, domain) {
  const headers = new Headers();
  for (const name of ['content-type', 'content-length', 'content-disposition', 'etag', 'last-modified']) {
    const value = source.get(name);
    if (value !== null) {
      headers.set(name, value);
    }
  }
  headers.set('cache-control', `public, max-age=${maxAge}${maxAge === PINNED_TTL ? ', immutable' : ''}`);
  headers.set('x-mirror', domain);
  headers.set('x-mirror-cache', cacheState);
  return headers;
}

function text(body, status, extra = {}) {
  return new Response(body, { status, headers: { 'content-type': 'text/plain; charset=utf-8', ...extra } });
}

function usage(configuration) {
  const domain = configuration.domain;
  const repository = configuration.origin?.href ?? DEFAULT_ORIGIN;
  const path = configuration.origin?.pathname ?? '/wty-yy/clash-verge-tui';
  return [
    `${domain} — GitHub release mirror for ${repository}`,
    '',
    'Supported URLs:',
    `  https://${domain}/${repository}/releases/download/<tag>/<asset>`,
    `  https://${domain}${path}/releases/download/<tag>/<asset>`,
    `  https://${domain}/install.sh`,
    `  https://${domain}/latest/<asset>`,
    `  https://${domain}/v<major>.<minor>.<patch>/<asset>`,
    `  https://${domain}/geodata/<geoip.dat|geosite.dat|geoip.metadb|country.mmdb|GeoLite2-ASN.mmdb>`,
    `  https://${domain}/ui/<metacubexd|yacd-meta>.tar.gz`,
    `  https://${domain}/core/v<version>/mihomo-linux-<arch>-v<version>.gz`,
    `  https://${domain}/latest-version`,
    '',
    'Install clash-verge-tui through this mirror:',
    `  curl -fsSL https://${domain}/install.sh | sh -s -- --source proxy`,
    '',
  ].join('\n');
}
