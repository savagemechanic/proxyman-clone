# Architecture

## Goals

Proxyman Clone is a clean-room, open-source network debugging workstation. The macOS product is split into a native SwiftUI client and a Rust networking engine so protocol/security code remains independent of the desktop UI.

## Components

### `proxy-core`

Pure Rust domain and protocol logic. It owns stable engine models, proxy state, capture models, interception rules, certificate abstractions, persistence interfaces, and other logic that should remain UI-independent.

### `proxy-daemon`

The executable host for the Rust engine. It owns listeners, async runtime integration, upstream networking, TLS interception, persistence wiring, and the local control API consumed by the desktop application.

### `ProxymanCloneApp`

Native SwiftUI macOS client. It owns windows, navigation, traffic views, inspectors, rule editors, setup flows, and platform integration. It does not implement proxy protocol logic.

## Control protocol

The first control boundary is a loopback-only TCP socket using newline-delimited JSON. It is intentionally small and explicit while the engine contracts are evolving.

Default endpoint: `127.0.0.1:9099`

Protocol version: `1`

Initial commands:

```json
{"type":"ping"}
{"type":"get_status"}
```

Initial responses:

```json
{"type":"pong","protocol_version":1}
{"type":"status","status":{"protocol_version":1,"proxy_state":"stopped","listen_address":null,"captured_transactions":0}}
```

The protocol must remain local-only by default. A future transport can replace TCP without coupling the UI to proxy internals.

## Data flow

```text
Configured client/device
        |
        v
  Rust proxy listener
        |
        +--> HTTP/TLS protocol pipeline
        |       |
        |       +--> interception/rules
        |       +--> upstream connection
        |
        +--> normalized capture events
                 |
                 +--> durable session store
                 +--> local control/event stream
                              |
                              v
                        SwiftUI desktop app
```

## Security invariants

- The engine listens only on loopback for its control API by default.
- TLS interception requires explicit user setup and trust of a local development CA.
- The CA private key stays local.
- Captured traffic is never telemetry.
- Sensitive headers and bodies are treated as secrets in logs and diagnostics.
- Remote-device proxy listeners must be explicitly enabled rather than silently exposed.

## Evolution

The Rust core should not assume SwiftUI. Future clients may include a CLI, Windows/Linux desktop clients, automated testing integrations, or a web-based local UI. Cross-platform interfaces therefore belong at the engine boundary, not inside the macOS app.
