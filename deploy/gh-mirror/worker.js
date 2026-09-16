// Cloudflare Worker: transparent GitHub release mirror for gh.wty-yy.top.
//
// Configured with two variables:
//   MIRROR_DOMAIN  public mirror hostname, e.g. gh.wty-yy.top
//   ORIGIN         upstream repository, e.g. https://github.com/wty-yy/clash-verge-tui
//
// Accepted URL forms (all resolve to <ORIGIN>/releases/...):
//   https://<MIRROR_DOMAIN>/https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>
//   https://<MIRROR_DOMAIN>/<owner>/<repo>/releases/download/<tag>/<asset>
//   https://<MIRROR_DOMAIN>/latest/<asset>
//   https://<MIRROR_DOMAIN>/v<major>.<minor>.<patch>/<asset>
//
// Only release assets of the configured repository are proxied; files are
// neither stored nor rewritten, responses are cached at the Cloudflare edge.
const DEFAULT_DOMAIN = 'gh.wty-yy.top';
const DEFAULT_ORIGIN = 'https://github.com/wty-yy/clash-verge-tui';
const PINNED_TTL = 31536000;
const LATEST_TTL = 300;
const REPOSITORY_PATH = /^\/[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/;
const RELEASE_SUFFIX = /^(?:download\/v[0-9]+\.[0-9]+\.[0-9]+\/|latest\/download\/)[A-Za-z0-9._-]+$/;
const SHORT_PATH = /^\/(latest|v[0-9]+\.[0-9]+\.[0-9]+)\/([A-Za-z0-9._-]+)$/;

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
    const target = configuration.origin === null ? null : resolveTarget(url.pathname, configuration.origin);
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

async function mirror(target, request, configuration, ctx) {
  const pinned = !target.pathname.includes('/latest/');
  const maxAge = pinned ? PINNED_TTL : LATEST_TTL;
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
    '',
    'Install clash-verge-tui through this mirror:',
    `  curl -fsSL ${repository}/releases/latest/download/install.sh | sh -s -- --source proxy`,
    '',
  ].join('\n');
}
