# TokenScope Hub

Go 1.23+ self-hosted synchronization and device registry service.

## Endpoints
- `GET /healthz`
- `POST /api/v1/pair`
- `POST /api/v1/usage/batch` (device-authenticated, max 200 records / 2 MiB)
- `GET /api/v1/devices`
- `PATCH /api/v1/devices/self`
- `POST /api/v1/devices/self/rotate-token`
- Admin: pairing-code creation and device revocation.

Usage uploads are partitioned by authenticated device ID and record ID. Revision increases replace prior payloads; equal revision/equal hash is idempotent; equal revision/different hash rejects the whole batch; lower revisions are returned as `superseded`.

See `../docs/M1-03-SYNC-PROTOCOL.md` for the wire contract.

## Self-hosted deployment

A Docker Compose + Caddy deployment template is provided in `../deploy` (M1-06). It exposes only ports 80/443 via Caddy; the Hub API stays on the private Docker network. See `../deploy/README.md` for DNS, TLS, pairing, backup/restore and security requirements.

## Change feed and remote analytics

`GET /api/v1/usage/changes?cursor=<non-negative>&limit=<1..500>` returns immutable Hub change-feed entries. Clients persist the payloads into their local remote cache and advance the cursor in the same local transaction. The desktop aggregates only `scope=device_local` records, preserving `owner_device_id` across tool/model/project dimensions. Quota snapshots and account-global/unknown usage are not attributed to a device.
