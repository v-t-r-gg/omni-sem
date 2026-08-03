# Local status HTTP service

Embedding projections come only from persisted schema v4 state: safe space/model metadata, dimensions, coverage, failures, and latest sync. The service never contacts Ollama and never exposes endpoint URLs, vectors, source/query text, or provider bodies.

The server receives the already-validated embedding configuration and reports network-free configured-versus-active compatibility. Mutable-tag digest verification remains the explicit responsibility of `doctor`.

## Bind

`omnisem status --serve [--port N]` listens on `127.0.0.1` only. Port `0` is ephemeral.

## Methods and routes

| Method | Routes |
|--------|--------|
| GET, HEAD | `/`, `/status.json`, `/healthz` |
| GET, HEAD | aliases `/health`, `/api/status`, `/api/roots`, `/api/activity` |

- Unsupported methods → `405 Method Not Allowed` + `Allow: GET, HEAD`
- Unknown path → `404`
- Malformed request line → `400`
- Oversized request headers → `431`

HEAD returns the same status and headers as GET with an empty body.

## Limits

Bounded request-line and header sizes, read timeout, one request per connection, no request body processing, connection close.

## Privacy

- Database opened read-only URI; migrations are not applied by the server.
- Response bodies never include source text or query strings.
- Internal SQLite/filesystem errors map to generic `500 internal error`.
- Absolute database/payload paths are not exposed in JSON fields intended for operators beyond what status already documents carefully; prefer counts and names.

## Headers

Includes at least:

```text
Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
Referrer-Policy: no-referrer
Cache-Control: no-store
```

No permissive CORS.

## Snapshot health

Status JSON includes registered, queryable, unhealthy payload counts, and mapped/unmapped root totals from the snapshot registry.
The status service remains persisted-state-only and provider-inert. Retrieval requested/effective modes and query vectors are not retained or exposed by the service.
