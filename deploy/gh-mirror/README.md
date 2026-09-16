# gh.wty-yy.top GitHub mirror

**English** · [简体中文](README.zh-CN.md)

`worker.js` is a transparent download proxy deployed on Cloudflare Workers: it serves this repository's GitHub Release assets (`install.sh`, `.tar.gz`, `.sha256`) through `https://gh.wty-yy.top` with edge caching. The script stores no files, calls no GitHub API, and forwards only release assets of the configured repository.

## Variables

| Variable | Example | Description |
| --- | --- | --- |
| `MIRROR_DOMAIN` | `gh.wty-yy.top` | Public mirror hostname, used in usage text and the `x-mirror` response header |
| `ORIGIN` | `https://github.com/wty-yy/clash-verge-tui` | Upstream repository; only assets under its `releases/` path are forwarded |

Both variables default to the values above. `wrangler.json` already fills `vars`; for Dashboard deployments enter the same two entries under Settings → Variables and Secrets.

## Supported URLs

| Form | Target |
| --- | --- |
| `/https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>` | Compatible with the gh-proxy prefix convention; `--github-proxy https://gh.wty-yy.top` works directly |
| `/<owner>/<repo>/releases/download/<tag>/<asset>` | Plain path without the prefix |
| `/install.sh` | Installer script of the latest release, cached 5 minutes |
| `/latest/<asset>` | GitHub resolves the latest release; cached 5 minutes |
| `/v<major>.<minor>.<patch>/<asset>` | Pinned version; cached 1 year |

`/health` returns `ok`, and `/` returns the usage text.

## Deployment

With the Dashboard:

1. Workers & Pages → Create application → Create Worker, name it `gh-mirror`.
2. Paste `worker.js` into Edit code and Deploy.
3. Settings → Domains & Routes → Add → Custom domain with `gh.wty-yy.top` (`wty-yy.top` is hosted on Cloudflare, so DNS records and certificates are created automatically; do not add A/CNAME records manually).
4. Settings → Variables and Secrets: add `MIRROR_DOMAIN` and `ORIGIN` (leave empty to keep the defaults).
5. Recommended: enable Always Use HTTPS under SSL/TLS.

With Wrangler:

```bash
# Run from deploy/gh-mirror; wrangler.json already contains both variables and the custom domain
npx wrangler login
npx wrangler deploy
```

## Verification

```bash
# Health check
curl -fsSL https://gh.wty-yy.top/health

# Download a checksum file (small, suitable for connectivity checks)
curl -fsSL https://gh.wty-yy.top/v1.5.0/clash-verge-tui-v1.5.0-linux-x86_64.tar.gz.sha256

# Install through the mirror: script and archives both use it; archives are still verified against the published SHA-256
curl -fsSL https://gh.wty-yy.top/install.sh | sh -s -- --source proxy
```

The release workflow writes the tag version into the published `install.sh` `release_version` during publish.

## Testing

```bash
node --test deploy/gh-mirror/*.test.mjs
```

## Notes

- The free Workers plan allows 100,000 requests per day; cache hits do not re-fetch GitHub. The first download goes upstream from Cloudflare, and later downloads are served from the edge.
- `/latest` paths are cached for 300 seconds and pinned version paths for 1 year, so repeated installs consume no GitHub bandwidth.
- Unmatched paths return 404; the Worker is not an open proxy.
