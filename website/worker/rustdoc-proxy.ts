/**
 * Cloudflare Worker: rustdoc JSON CORS proxy
 *
 * Proxies rustdoc JSON from docs.rs with CORS headers so the RustScript
 * crate docs viewer can fetch it client-side.
 *
 * Routes:
 *   /crate/{name}/{version}/json.gz  → docs.rs/crate/{name}/{version}/json.gz
 *
 * Caching:
 *   - Specific versions (e.g., /crate/axum/0.8.1/json.gz) → cached 1 year
 *     (published crate versions are immutable on crates.io)
 *   - "latest" (e.g., /crate/axum/latest/json.gz) → cached 1 hour
 *     (so new releases are picked up within an hour)
 *
 * Deploy: wrangler deploy worker/rustdoc-proxy.ts
 */

export interface Env {}

export default {
  async fetch(request: Request, _env: Env, _ctx: ExecutionContext): Promise<Response> {
    // Handle CORS preflight
    if (request.method === 'OPTIONS') {
      return new Response(null, {
        headers: {
          'Access-Control-Allow-Origin': '*',
          'Access-Control-Allow-Methods': 'GET, OPTIONS',
          'Access-Control-Allow-Headers': 'Content-Type',
          'Access-Control-Max-Age': '86400',
        },
      });
    }

    if (request.method !== 'GET') {
      return new Response('Method not allowed', { status: 405 });
    }

    const url = new URL(request.url);
    const path = url.pathname;

    // Only proxy /crate/* paths
    if (!path.startsWith('/crate/')) {
      return new Response('Not found. Use /crate/{name}/{version}/json.gz', {
        status: 404,
      });
    }

    // Check edge cache first
    const cache = caches.default;
    const cached = await cache.match(request);
    if (cached) {
      return cached;
    }

    // Proxy to docs.rs
    const docsUrl = `https://docs.rs${path}`;
    const upstream = await fetch(docsUrl, {
      headers: { 'User-Agent': 'rustscript-docs-proxy/1.0' },
    });

    if (!upstream.ok) {
      return new Response(`docs.rs returned ${upstream.status}`, {
        status: upstream.status,
        headers: { 'Access-Control-Allow-Origin': '*' },
      });
    }

    // Build response with CORS headers
    const headers = new Headers(upstream.headers);
    headers.set('Access-Control-Allow-Origin', '*');

    // Cache policy: immutable versions forever, "latest" for 1 hour
    const isLatest = path.includes('/latest/');
    const ttl = isLatest ? 3600 : 31536000; // 1 hour vs 1 year
    headers.set('Cache-Control', `public, max-age=${ttl}`);

    const response = new Response(upstream.body, {
      status: upstream.status,
      headers,
    });

    // Store in edge cache (non-blocking)
    _ctx.waitUntil(cache.put(request, response.clone()));

    return response;
  },
} satisfies ExportedHandler<Env>;
