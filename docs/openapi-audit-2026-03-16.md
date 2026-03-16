# OpenAPI/utoipa Audit – Strom

**Datum:** 2026-03-16
**Syfte:** Kartlägga hur strikt API-kontraktet är idag och vad som krävs för att externa konsumenter (t.ex. Open Live) ska kunna bygga på det.

## 1. Täckningsgrad

**Before (audit):**

| Kategori | Antal | Andel |
|----------|-------|-------|
| Endpoints med `#[utoipa::path]` | 68 | 72 % av 94 routes |
| Endpoints registrerade i `openapi.rs` | 47 | 50 % av 94 routes |
| Annoterade men **saknas i openapi.rs** | 24 | 26 % |
| Helt utan annotation | 26 | 28 % |

**After (fixed):**

| Kategori | Antal | Andel |
|----------|-------|-------|
| Endpoints med `#[utoipa::path]` | 83 | 88 % av 94 routes |
| Endpoints registrerade i `openapi.rs` | 83 | 88 % av 94 routes |
| Annoterade men saknas i `openapi.rs` | 0 | 0 % |
| Helt utan annotation | 11 | 12 % |

### Fixed
- 24 previously annotated but unregistered endpoints now registered in `openapi.rs` (discovery, media player, flow endpoints)
- 12 WHIP/WHEP proxy endpoints annotated and registered
- 30 schema types added to `components(schemas(...))`

### Remaining gaps
- `health` endpoint – intentionally undocumented
- 3 HTML page routes (`whep_player`, `whep_streams_page`, `whip_ingest_page`) – serve HTML, not JSON
- 6 static asset routes – CSS/JS files, not API endpoints
- ~10 discovery endpoints still use `impl IntoResponse` (annotations specify types explicitly, but signatures are not compile-time checked)

## 2. Valideringsstatus

| Aspekt | Before | After |
|--------|--------|-------|
| Validerings-crate | None | **`garde` added** – derives on 10 request types with constraints (non-empty, length limits, duration ranges) |
| Custom Axum-extractors | None | **`JsonBody<T>` added** – returns structured JSON errors on deserialization failures |
| Global rejection handler | Missing | **Available** – `JsonBody<T>` can be adopted incrementally per handler |
| Strukturerade felsvar i handlers | Yes | Yes |
| Path traversal-skydd | Yes | Yes |
| Query param-validering | Partial | Partial |
| Body size limit | Upload only | Upload only |
| Auth-validering | Yes | Yes |

### Remaining work
- `JsonBody<T>` is available but not yet wired into existing handlers (requires changing `Json<T>` to `JsonBody<T>` per handler)
- `garde::Validate` derives are in place but `validate()` is not yet called in handlers – needs a middleware or per-handler call
- External consumers sending malformed JSON still get Axum's default plaintext on handlers using `Json<T>`

## 3. WebSocket-kontraktsstatus

| Aspekt | Before | After |
|--------|--------|-------|
| Endpoint | Registered | Registered |
| Event-typ | `StromEvent`, 33 variants | No change |
| Serialisering | JSON tagged enum | No change |
| ToSchema-annotation | **Missing** | **Added** on `StromEvent`, `ServerMessage`, `ClientMessage` |
| Formell schemadefinition | None | **Event types now in OpenAPI components** |
| Versionshantering | None | None |
| Riktning | Server → client | No change |

### Remaining work
- No versioning mechanism for WebSocket events
- Breaking change rules documented in `CLAUDE.md` but not enforced at CI level

## 4. ToSchema-gaps

### Previously missing – now fixed

| Typ | Status |
|-----|--------|
| `StromEvent` | **Fixed** – `ToSchema` added |
| `ServerMessage` | **Fixed** – `ToSchema` added |
| `ClientMessage` | **Fixed** – `ToSchema` added |

### Serde-attribut med schemarisk (unchanged)

| Attribut | Typ | Risk |
|----------|-----|------|
| `#[serde(untagged)]` | `PropertyValue` (`element.rs:136`) | Schema genererar `oneOf` utan discriminator |
| Custom `Deserialize` impl | `CpuAffinity` (`flow.rs:31`) | Schemat reflekterar inte custom parsing-logik |

### Backend-typer utanför `strom-types` (unchanged)

Discovery-typer (`DiscoveredStreamResponse`, `DeviceDiscoveryStatus`, etc.) och mediaplayer-typer (`PlayerControlRequest`, `SeekRequest`, etc.) ligger i backend men exponeras via REST.

## 5. CI-skydd

| Skyddsmekanism | Before | After |
|----------------|--------|-------|
| OpenAPI snapshot-test | Missing | **Added** – `openapi_snapshot_test.rs` |
| OpenAPI-spec i repo | Missing | **Added** – `openapi_snapshot.json` |
| Schema-diff i CI | Missing | **Added** – `oasdiff` job in CI (PRs only) |
| Breaking-change detection | Missing | **Added** – `oasdiff breaking` fails CI on breaking changes |
| Integrationstester mot schema | Missing | Missing |
| Pre-commit hook | No schema check | No schema check |

### How it works now
- `cargo test --test openapi_snapshot_test` catches any spec drift
- CI runs `oasdiff` on PRs to detect breaking changes and generate a changelog
- Contract rules documented in `CLAUDE.md`

## 6. Prioriterad åtgärdslista

### Fas 1 – Minimalt skydd (DONE)

| # | Åtgärd | Status |
|---|--------|--------|
| 1 | Registrera 24 saknade endpoints i `openapi.rs` | Done |
| 2 | Global rejection handler (`JsonBody<T>`) | Done |
| 3 | Snapshot-test + incheckad `openapi_snapshot.json` | Done |

### Fas 2 – Externt konsumerbart kontrakt (DONE)

| # | Åtgärd | Status |
|---|--------|--------|
| 4 | `ToSchema` på `StromEvent`, `ServerMessage`, `ClientMessage` | Done |
| 5 | Annotera WHIP/WHEP-proxy-endpoints | Done |
| 6 | `garde` validation derives på request-typer | Done |
| 7 | `oasdiff` breaking-change detection i CI | Done |
| 8 | OpenAPI version från `CARGO_PKG_VERSION` | Done |
| 9 | Contract rules i `CLAUDE.md` | Done |

### Fas 3 – Robust kontrakt (TODO)

| # | Åtgärd | Impact | Insats |
|---|--------|--------|--------|
| 10 | Wire `garde::Validate` into request pipeline | Validation actually enforced at runtime | Medel |
| 11 | Migrate handlers from `Json<T>` to `JsonBody<T>` | Consistent JSON error responses for all endpoints | Medel |
| 12 | Flytta discovery/mediaplayer-typer till `strom-types` | Klientgenerering fungerar utan backend-access | Medel |
| 13 | Versionering av WebSocket-event | Bakåtkompatibilitet vid event-ändringar | Stor |
| 14 | Integrationstester som validerar response mot schemat | Schemat bevisas korrekt, inte bara deklarativt | Stor |
