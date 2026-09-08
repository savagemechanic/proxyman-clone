# TLS interception architecture

TLS interception is an MVP requirement for Proxyman Clone.

This document defines the security and implementation boundaries for the HTTPS MITM layer.

## Goals

- Generate a local development Certificate Authority (CA) on first use.
- Persist the CA certificate and private key only on the local machine.
- Generate and cache per-host leaf certificates signed by the local CA.
- Intercept HTTPS only for clients the user explicitly configures to trust the CA.
- Keep certificate state and captured secrets out of telemetry.
- Preserve an explicit bypass path for hosts the user does not want intercepted.

## Flow

```text
client
  |
  | CONNECT example.com:443
  v
proxy daemon
  |
  | 200 Connection Established
  v
TLS acceptor (leaf cert for example.com)
  |
  | decrypted HTTP
  v
capture/rules pipeline
  |
  | independent TLS client connection
  v
example.com:443
```

The proxy terminates the client-side TLS session locally, then creates a separate authenticated TLS connection to the upstream host. The upstream certificate remains validated using the platform/web PKI roots.

## Local CA storage

The daemon resolves its application data directory in this order:

1. `PROXYMAN_CLONE_DATA_DIR` when explicitly configured.
2. `$HOME/Library/Application Support/ProxymanClone` on macOS.
3. A platform-appropriate local application-data fallback.

Files:

```text
ca/
  proxyman-clone-ca.pem
  proxyman-clone-ca-key.pem
cert-cache/
```

The private key must never be printed to logs or exposed over the control protocol.

## Trust model

Creating a CA does not make it trusted automatically. The user must explicitly install/trust the exported CA certificate on each development device.

The macOS UI will eventually provide guided installation and removal. The Rust layer is responsible for generation, persistence, and export paths, not silently mutating trust stores.

## Bypass model

CONNECT interception should support an allow/bypass decision before performing the TLS handshake. Initial implementation can use environment/config patterns, with the later rules UI controlling the same policy.

## Certificate cache

Leaf certificates are deterministic only in identity, not key material. The proxy can cache generated leaf certificate/key pairs by canonical hostname to avoid regenerating them for every connection.

Cache entries must:

- include SAN entries for the requested hostname/IP where valid,
- have a short validity period,
- be signed by the local CA,
- be invalidated when the CA changes.

## Security invariants

- CA private key never leaves local storage.
- No CA key material in logs, crash telemetry, issues, or session exports.
- Interception defaults to loopback proxy listeners unless explicitly reconfigured.
- Upstream certificate verification is never disabled merely because downstream interception is enabled.
- TLS failures fail closed and surface a useful error rather than silently downgrading to plaintext.
