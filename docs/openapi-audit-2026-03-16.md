# OpenAPI/utoipa Audit – Strom

**Datum:** 2026-03-16
**Syfte:** Kartlägga hur strikt API-kontraktet är idag och vad som krävs för att externa konsumenter (t.ex. Open Live) ska kunna bygga på det.

## 1. Täckningsgrad

| Kategori | Antal | Andel |
|----------|-------|-------|
| Endpoints med `#[utoipa::path]` | 68 | 72 % av 94 routes |
| Endpoints registrerade i `openapi.rs` | 47 | 50 % av 94 routes |
| Annoterade men **saknas i openapi.rs** | 24 | 26 % |
| Helt utan annotation | 26 | 28 % |

### Största luckorna

- **Discovery API** (13 endpoints) – alla annoterade, **ingen registrerad** i `openapi.rs`
- **Media Player API** (5 endpoints) – alla annoterade, **ingen registrerad** i `openapi.rs`
- **Flow-endpoints** (8 st) – annoterade men saknas i `openapi.rs`: `get_available_sources`, `get_block_sdp`, `get_flow_latency`, `get_webrtc_stats`, `get_pad_properties`, `update_pad_property`, `reset_loudness`, `recorder_split_now`
- **WHIP proxy** (5 endpoints) + **WHEP proxy** (5 endpoints) – helt utan annotation
- **`list_whip_endpoints`**, **`client_log`**, **`health`** – utan annotation

Request/response-typer är väl beskrivna för de endpoints som är registrerade. ~10 discovery-endpoints returnerar `impl IntoResponse` (utoipa-makron anger explicit typ, men signaturen ger ingen compile-time-garanti).

## 2. Valideringsstatus

| Aspekt | Status |
|--------|--------|
| Validerings-crate (`validator`, `garde`) | **Ingen** – serde-deserialisering är enda gaten |
| Custom Axum-extractors | **Inga** – standard `Json<T>`, `Path<T>`, `Query<T>` |
| Global rejection handler | **Saknas** – felaktig JSON ger Axums default plaintext-svar, inte strukturerad JSON |
| Strukturerade felsvar i handlers | **Ja** – `ErrorResponse { error, details? }` med korrekta statuskoder |
| Path traversal-skydd | **Ja** – `validate_path()` med canonicalization i media-API |
| Query param-validering | **Delvis** – `clamp()` för thumbnail-dimensioner |
| Body size limit | **Bara upload** – 500 MB för `/api/media/upload`, resten Axum default |
| Auth-validering | **Ja** – session, Bearer token, API-key, origin-kontroll (MCP) |

En extern konsument som skickar felformaterad JSON får Axums default plaintext-rejection – inte den `ErrorResponse`-JSON som dokumenteras i OpenAPI-schemat.

## 3. WebSocket-kontraktsstatus

| Aspekt | Status |
|--------|--------|
| Endpoint | `GET /api/ws` – registrerad i OpenAPI |
| Event-typ | `StromEvent` enum, 33 varianter, definierad i `strom-types` |
| Serialisering | JSON med `#[serde(tag = "type", content = "data")]` |
| ToSchema-annotation | **Saknas** på `StromEvent`, `ServerMessage`, `ClientMessage` |
| Formell schemadefinition | **Ingen** – inga event-typer i OpenAPI-komponenter |
| Versionshantering | **Ingen** – inget versionsfält, ingen negotiation |
| Riktning | Primärt server → klient (broadcast), klient skickar bara `"ping"` |

WebSocket-kontraktet lever helt utanför OpenAPI. En extern konsument måste reverse-engineera event-formatet från källkoden.

## 4. ToSchema-gaps

### Typer som saknar `ToSchema` men är API-synliga

| Typ | Fil | Användning |
|-----|-----|-----------|
| `StromEvent` | `types/src/events.rs:12` | WebSocket-broadcast |
| `ServerMessage` | `types/src/api.rs:241` | WebSocket-svar |
| `ClientMessage` | `types/src/api.rs:264` | WebSocket-meddelanden |

### Serde-attribut med schemarisk

| Attribut | Typ | Risk |
|----------|-----|------|
| `#[serde(untagged)]` | `PropertyValue` (`element.rs:136`) | Schema genererar `oneOf` utan discriminator |
| Custom `Deserialize` impl | `CpuAffinity` (`flow.rs:31`) | Schemat reflekterar inte custom parsing-logik |

### Backend-typer utanför `strom-types`

Discovery-typer (`DiscoveredStreamResponse`, `DeviceDiscoveryStatus`, etc.) och mediaplayer-typer (`PlayerControlRequest`, `SeekRequest`, etc.) ligger i backend men exponeras via REST.

## 5. CI-skydd

| Skyddsmekanism | Status |
|----------------|--------|
| OpenAPI snapshot-test | **Saknas** |
| Schema-diff i CI | **Saknas** |
| Integrationstester mot schema | **Saknas** |
| OpenAPI-spec i repo | **Saknas** – genereras runtime |
| Pre-commit hook | **Ingen schema-kontroll** |

En utvecklare kan ändra en response-typ eller lägga till en endpoint utan att det syns i schemat eller bryter builden.

## 6. Prioriterad åtgärdslista

### Fas 1 – Minimalt skydd

| # | Åtgärd | Impact | Insats |
|---|--------|--------|--------|
| 1 | Registrera 24 saknade endpoints i `openapi.rs` | Specen beskriver hela REST-API:et | Liten |
| 2 | Global rejection handler för konsistent JSON-felsvar | Alla felsvar matchar `ErrorResponse`-schemat | Liten |
| 3 | Generera och checka in `openapi.json` + snapshot-test i CI | Vaktar kontraktet | Liten |

### Fas 2 – Externt konsumerbart kontrakt

| # | Åtgärd | Impact | Insats |
|---|--------|--------|--------|
| 4 | `ToSchema` på `StromEvent`, `ServerMessage`, `ClientMessage` | Event-typerna schema-definierade | Medel |
| 5 | Dokumentera WebSocket-kontraktet | Extern konsument kan generera klientkod | Medel |
| 6 | Annotera WHIP/WHEP-proxy-endpoints | Schemat har inga "hemliga" endpoints | Liten–Medel |

### Fas 3 – Robust kontrakt

| # | Åtgärd | Impact | Insats |
|---|--------|--------|--------|
| 7 | `garde`/`validator` på request-typer | Runtime-validering matchar schemat | Medel |
| 8 | Flytta discovery/mediaplayer-typer till `strom-types` | Klientgenerering fungerar utan backend-access | Medel |
| 9 | Versionering av WebSocket-event | Bakåtkompatibilitet vid event-ändringar | Stor |
| 10 | Breaking-change-detection i CI (`oasdiff`) | PR-review ser API-påverkan | Medel |
