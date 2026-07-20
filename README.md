# cautious-waffle

A small HTTP service that updates Cloudflare `A` records — a dynamic DNS backend.

Clients authenticate with a UUID in the URL path and report an IP address. The server maps
that UUID to one or more hostnames, resolves each hostname to its Cloudflare zone, and
updates the record when the address has actually changed.

It can also run in **relay mode**, where instead of talking to Cloudflare it forwards updates
to another upstream instance. This is useful when the machine facing the client cannot hold
the Cloudflare API token.

## Features

- Updates Cloudflare `A` records via the v4 API
- Listens on any number of addresses, so IPv4 and IPv6 can be served together
- One UUID may map to multiple hostnames across multiple zones
- Automatic zone resolution by matching the longest domain suffix
- Takes the IP from a request header (`X-Real-IP` by default) or from a JSON body
- Relay mode with optional outbound proxy (HTTP / SOCKS)
- Live configuration reload — edits to the config file are picked up without a restart
- Graceful shutdown on `Ctrl-C`

## Building

```sh
cargo build --release
```

The binary is written to `target/release/cautious-waffle`.

## Configuration

Copy the template and edit it:

```sh
cp config.toml.default config.toml
```

```toml
token = "CF_TOKEN"       # Cloudflare API token, needs DNS edit rights on the zones below
column_ip = "X-Real-IP"  # optional, header carrying the client IP (default: X-Real-IP)

[server]
host = "127.0.0.1"       # one address, or several: ["127.0.0.1", "::1"]
port = 21336
enable_query = false     # optional, see "Query endpoint" below

# A client: one UUID, one or more hostnames to keep pointed at its IP.
[[client]]
uuid = "db5770ec-750b-4dd9-9fe2-2bf374b0ab50"
target = ["test.example.com"]

# Maps a domain to its Cloudflare zone ID. Hostnames above are matched
# against these by longest suffix, so one entry covers a whole zone.
[[zones]]
domain = "example.com"
zone = "fbdda469ff654a13826ed0222cc30aba"
```

Outside relay mode, `token`, at least one `[[zones]]` entry and at least one `[[client]]`
entry are all required; the server refuses to start otherwise.

Note that `config.toml` is listed in `.gitignore`, since it holds an API token.

### Relay mode

With `relay.enabled = true` the server never contacts Cloudflare. Incoming updates are
POSTed to each upstream in `target` until one succeeds, and `token` and `[[zones]]` become
unnecessary.

```toml
[relay]
enabled = true
target = ["https://example.com/"]
proxy = "socks5://127.0.0.1:1080"  # optional

# Relay clients are rewritten: a request for `uuid` is forwarded as `target`.
# If `target` is omitted the incoming UUID is passed through unchanged.
[[relay.clients]]
uuid = "7e42b2ee-38bc-4f49-9ae0-ed527952ac5c"
target = "db5770ec-750b-4dd9-9fe2-2bf374b0ab50"
```

Each upstream URL is concatenated with the UUID directly, so it should end with `/`, `=`
or `?`. The server warns on startup if one does not; set `DISABLE_URL_WARNING=1` to silence
that.

## Running

```sh
cautious-waffle --config config.toml
```

| Flag | Description |
| --- | --- |
| `--config <file>` | Configuration file location (default: `config.toml`) |
| `--disable-watcher` | Do not reload the configuration when the file changes |
| `--enable-query` | Enable the `/query` endpoint |
| `--systemd` | Plain log format, without timestamps, for journald |

Log level follows `env_logger`, so use `RUST_LOG=debug` for verbose output.

## Listen addresses

To accept updates over both stacks, give `host` a list:

```toml
[server]
host = ["0.0.0.0", "::"]
port = 21336
```

Each address gets its own listener sharing one router, and `Ctrl-C` shuts all of them
down together. IPv6 addresses may be written with or without brackets (`::1` or `[::1]`).
Note that on most Linux systems `::` alone already accepts IPv4 connections via
`net.ipv6.bindv6only=0`, in which case listing `0.0.0.0` as well will fail with "address
already in use".

## API

### `GET /<uuid>`

Updates the records for `<uuid>` using the IP in the `X-Real-IP` header. Intended to sit
behind a reverse proxy that sets the header:

```nginx
proxy_set_header X-Real-IP $remote_addr;
```

```sh
curl -H 'X-Real-IP: 203.0.113.10' http://127.0.0.1:21336/db5770ec-750b-4dd9-9fe2-2bf374b0ab50
```

### `POST /<uuid>`

Supplies the address explicitly, which is useful when the client knows its own public IP:

```sh
curl -X POST -H 'Content-Type: application/json' \
     -d '{"ip": "203.0.113.10"}' \
     http://127.0.0.1:21336/db5770ec-750b-4dd9-9fe2-2bf374b0ab50
```

### `GET /`

Returns the running version.

Responses are `200` on success, `400` if the UUID is malformed, `403` if it is unknown or
no IP could be determined, and `503` if a relay failed to reach any upstream. A `200` means
the request was accepted — it does not imply a record was rewritten, since unchanged
addresses are skipped. Any other path returns `403`.

### Query endpoint

With `--enable-query` (or `enable_query = true`), `GET /query` echoes the request headers
back as JSON, which helps when working out which header carries the real client IP. It
reflects whatever a caller sends and is unauthenticated, so it is meant for temporary
debugging rather than for exposure to the internet. The server logs a warning while it is
enabled; set `DISABLE_QUERY_WARNING=1` to suppress that.

## Configuration reload

The configuration file is watched by default, and the client and zone mappings are rebuilt
in place when it changes. A config that fails to parse is logged and ignored, leaving the
previous one in effect. The listen address is applied only at startup, so changing
`[server]` still requires a restart. Pass `--disable-watcher` to turn watching off.

Switching `relay.enabled` is best done with a restart rather than a reload; see
[Known issues](#known-issues).

## Known issues

- Toggling `relay.enabled` through a configuration reload does not fully take effect. The
  request handler keeps using the previous mode when deciding whether to read the client IP
  from the request header, so a reload that enables or disables relay mode can leave the
  server in an inconsistent state until it is restarted.

## License

[![](https://www.gnu.org/graphics/agplv3-155x51.png)](https://www.gnu.org/licenses/agpl-3.0.txt)

Copyright (C) 2023-2026 KunoiSayami

This program is free software: you can redistribute it and/or modify it under the terms of the
GNU Affero General Public License as published by the Free Software Foundation,
either version 3 of the License, or any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
See the GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License along with this program.
If not, see <https://www.gnu.org/licenses/>.