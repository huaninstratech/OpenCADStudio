# `ocs_web` — self-hosted signing in, and drawings behind it

A small server that turns the web build into a private drawing viewer: it
signs users in against an OpenID Connect provider and serves the drawings
each user may see. One static binary, one folder of the web build, one folder
of drawings.

## Why a server is needed

The app is a WebAssembly page: it can fetch a drawing by URL, but it cannot
sign anybody in on its own, for two reasons:

1. **The provider does not allow cross-origin calls on its token endpoint.**
   The page could send the browser to `connect/authorize`, but it could not
   turn the returned code into a token — the browser would refuse the
   response for lack of `Access-Control-Allow-Origin`.
2. **A client secret must not be in a page.** Anyone can read a page's
   JavaScript.

So the code-for-token exchange happens here, and the browser only ever holds
an opaque session cookie. That cookie is `HttpOnly`, so no script can read
it, and `SameSite=Lax`, so another site cannot spend it.

Because the page and the drawings come from the same origin, the app's
existing download path works unchanged: `?file=/drawings/<id>` carries the
cookie automatically. No change to the WebAssembly client is needed.

## What the server answers

| Address | What it does |
|---|---|
| `GET /` | the web build (`index.html`) |
| `GET /auth/login` | starts a sign-in and sends the browser to the provider |
| `GET /auth/callback` | the address the provider sends the browser back to |
| `POST /auth/logout` | ends the session here (403 from another site) |
| `GET /auth/me` | who is signed in, as JSON, for the interface |
| `GET /drawings` | the drawings this user may open, as JSON |
| `GET /drawings/<id>` | one drawing's bytes — the `?file=` address |
| `GET /healthz` | `{"status":"ok","sessions":N,"bundle_files":N}` |

## Setting it up

### 1. Register the client at the provider

At the provider's administration (this repository's reference provider is
`https://amecc.spm.io.vn`), create an OpenID Connect client:

| Setting | Value |
|---|---|
| Client type | confidential (has a secret) or public |
| Grant types | `authorization_code` (with PKCE) |
| Redirect URI | `https://<your host>/auth/callback` |
| Scopes | `openid`, `profile`, `email` |
| Token endpoint auth | `client_secret_post` (the provider's default) |

The redirect URI must match exactly, including the path
`/auth/callback` — the server refuses to start otherwise.

### 2. Put the files in place

```text
/srv/ocs/
├── app/                 the web build: `trunk build` output (index.html, *.wasm, …)
├── drawings/            the drawings
│   ├── du-an-a/
│   │   ├── ban-ve-01.dxf
│   │   ├── ban-ve-02.dwg
│   │   └── access       (optional) the roles that may see this collection
│   └── du-an-b/
│       ├── chi-tiet.dxf
│       └── access       `viewer`
├── ocs_web              this binary
└── ocs_web.env          the settings below
```

## Where the drawings come from

Two sources, chosen by the settings:

| Setting | Source |
|---|---|
| `OCS_FILE_SERVICES_URL` | **INS.FileServices** — the suite's file service. The server lists `GET /api/files?ownerModule=<module>` and streams `GET /api/files/{id}/content`, forwarding **the user's own OIDC access token**. The service's own permissions (`files:read`) decide what is visible; this server never overrules them. |
| `OCS_DRAWINGS_DIR` | a folder on this machine (one subfolder per collection, optional `access` file) |

When both are set, the file service wins. `OCS_FILE_SERVICES_MODULE` names the
`ownerModule` the drawings are filed under (default `OCS`).

For the file service to accept the forwarded token, it must trust the same
provider `OCS_OIDC_ISSUER` names — that is a deployment fact on its side, not
something this server can arrange.

### The folder source, when a folder is used

`access` holds one role per line (or comma separated). A collection **with**
an `access` file is visible only to a user holding one of those roles; a
collection **without** one is visible to every signed-in user (an
operator's own folder). Administrators see everything.

Drawings directly in `drawings/` (not in a collection) are visible to every
signed-in user.

Only `.dwg`, `.dxf`, `.ifcx` and `.ifccad` are offered — the same set the
client's `?file=` link opens.

### 3. The settings

`ocs_web.env`:

```sh
# The provider. Its endpoints are found through
# /.well-known/openid-configuration.
OCS_OIDC_ISSUER=https://amecc.spm.io.vn
OCS_OIDC_CLIENT_ID=ocs-web
OCS_OIDC_CLIENT_SECRET=<the client's secret>
# Must end in /auth/callback and match what was registered.
OCS_OIDC_REDIRECT_URI=https://ssdnode.example/auth/callback
# Optional; these are the defaults.
OCS_OIDC_SCOPES="openid profile email"

# What to serve.
OCS_STATIC_DIR=/srv/ocs/app
# Drawings from the suite's file service (wins over the folder below):
OCS_FILE_SERVICES_URL=http://127.0.0.1:10319
OCS_FILE_SERVICES_MODULE=OCS
# …or from a folder on this machine:
# OCS_DRAWINGS_DIR=/srv/ocs/drawings

# Where to listen. Behind a reverse proxy, keep it on loopback.
OCS_LISTEN=127.0.0.1:8080

# Roles that see every collection. `roles:ocs-admin` reads the provider's
# `roles` claim; a bare name matches any of the user's roles.
OCS_ADMIN_CLAIMS="roles:ocs-admin"

# Leave this on in any real deployment: the cookie then travels only over
# HTTPS. Turn it off only for a loopback test.
OCS_COOKIE_SECURE=1
```

Set `OCS_STATIC_DIR` to serve the build from this server, or leave it unset
to serve drawings only while a reverse proxy serves the build. At least one
of `OCS_STATIC_DIR` and `OCS_DRAWINGS_DIR` must be set.

### 4. A service unit

```ini
# /etc/systemd/system/ocs-web.service
[Unit]
Description=Open CAD Studio web viewer
After=network-online.target

[Service]
Type=simple
User=ocs
Group=ocs
WorkingDirectory=/srv/ocs
EnvironmentFile=/srv/ocs/ocs_web.env
ExecStart=/srv/ocs/ocs_web
Restart=on-failure
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ReadOnlyPaths=/srv/ocs/drawings
ProtectHome=yes

[Install]
WantedBy=multi-user.target
```

### 5. TLS in front

The provider requires an HTTPS redirect URI. A reverse proxy terminates TLS
and passes the cookie through:

```nginx
server {
    listen 443 ssl http2;
    server_name ssdnode.example;

    ssl_certificate     /etc/letsencrypt/live/ssdnode.example/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/ssdnode.example/privkey.pem;

    # Drawings can be large; the client refuses above 64 MB.
    client_max_body_size 0;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host              $host;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

`Host` must be passed through: the logout endpoint compares it against the
browser's `Origin` to refuse cross-site requests.

Behind Cloudflare the origin certificate is the `CloudFlare Origin Certificate`
(`*.spmcloud.io`), and the vhost binds the server's public address rather than
`0.0.0.0` — a host that also runs WireGuard and Tailscale should not answer the
world on every interface:

```nginx
server {
    listen 209.182.237.162:443 ssl;
    http2 on;
    server_name cad.spmcloud.io;

    ssl_certificate     /home/huanld/ins-vault/cert/amecc-spmcloud-origin.crt;
    ssl_certificate_key /home/huanld/ins-vault/cert/amecc-spmcloud-origin.key;

    location / {
        proxy_pass http://127.0.0.1:18080;
        proxy_http_version 1.1;
        proxy_set_header Host              $host;
        proxy_set_header X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Upgrade           $http_upgrade;
        proxy_set_header Connection        "upgrade";
        proxy_read_timeout 600;
        proxy_buffering off;
    }
}
```

Plain HTTP on 80 still needs to be allowed through the firewall: Cloudflare
follows the redirect, and an unreachable port 80 shows up as 522 rather than a
TLS error. Do not set `sslFlags`/SNI games on Linux — binding the address and
letting `server_name` select the certificate is enough.

## Using it from the interface

Once signed in, the app opens a private drawing with the address it already
understands:

```text
https://ssdnode.example/?ui=viewer&file=/drawings/du-an-b/chi-tiet.dxf
```

The cookie travels with that fetch because the address is on the same server.
`file=` accepts a same-origin path here; the client's rule that a `?file=`
value ending in a drawing name may be a URL covers it, since the path ends in
`.dxf`.

To list what is available, `GET /drawings` answers:

```json
{"drawings":[{"id":"du-an-b/chi-tiet.dxf","name":"chi-tiet.dxf",
              "collection":"du-an-b","bytes":48213}]}
```

## Security notes

- **Nothing about a refusal is detailed.** A drawing that does not exist and
  one the user may not see answer the same 404.
- **Path traversal is refused.** A collection or file name that is not one
  plain path component (`..`, separators, a leading `.`) never reaches disk.
- **Sessions live in memory.** A restart signs everyone out — there is no
  session database to go stale, and no session survives the process that
  could revoke it. A session is renewed by use and dropped after 12 hours.
- **The provider's session is not ended on logout**, only this server's. One
  viewer signing out should not sign the user out of the rest of the suite.
- **The `id_token` payload is read without re-checking its signature.** It
  arrived over TLS straight from the token endpoint, which is the same
  channel the code was sent on; nothing else could have written it. A
  deployment that needs signature checking as well would verify it against
  the provider's JWKS — that is a deliberate next step, not an oversight.

## Building it

```sh
cargo build -p ocs_web --release --locked
```

The binary is `target/release/ocs_web` (`ocs_web.exe` on Windows). Nothing in
it compiles to WebAssembly, and it does not link the geometry kernel.
