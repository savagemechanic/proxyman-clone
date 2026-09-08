# TLS interception

Proxyman Clone can intercept HTTPS traffic from development clients that you explicitly configure and authorize.

TLS interception is **off by default**. When disabled, `CONNECT` requests are passed through as opaque TCP tunnels.

## Enable interception

Start the Rust engine with:

```bash
PROXYMAN_CLONE_TLS_INTERCEPT=1 cargo run -p proxy-daemon
```

The proxy still listens on `127.0.0.1:8080` by default. The control API remains on `127.0.0.1:9099`.

On macOS, the generated development CA is stored by default under:

```text
~/Library/Application Support/ProxymanClone/certificates/
```

The directory contains:

- `ca-cert.pem` — public CA certificate that may be installed on an authorized test device
- `ca-key.pem` — private CA key; this must remain local and is written with owner-only permissions on Unix systems

Set `PROXYMAN_CLONE_CERT_DIR` to override the certificate directory for development or tests.

## Trust the CA on macOS

Only do this on a machine you control and only while you need HTTPS inspection.

1. Start the engine once with TLS interception enabled so the CA is generated.
2. Open **Keychain Access**.
3. Import `ca-cert.pem` into the login keychain.
4. Open the imported certificate, expand **Trust**, and choose the trust setting appropriate for your local development workflow.
5. Configure the application or device you are debugging to use the HTTP proxy at the engine's proxy address.

Remove the certificate from Keychain when you no longer need interception.

## How it works

For an intercepted `CONNECT host:443` session, the engine:

1. acknowledges the proxy tunnel request;
2. creates or reuses a short-lived in-memory leaf identity for the requested hostname;
3. accepts a TLS connection from the configured development client using that leaf certificate;
4. independently establishes a normal TLS connection to the real upstream host using the system-compatible WebPKI root set and SNI;
5. relays decrypted application bytes between the two authenticated TLS sessions.

The generated leaf certificates are signed by the local development CA and cached only in memory. The CA itself persists locally so a development device does not need to re-trust a new authority on every engine restart.

## Security boundaries

- TLS interception is opt-in rather than automatic.
- The CA private key is never transmitted by the engine.
- The control listener and default proxy listener bind to loopback.
- The project contains no traffic telemetry upload path.
- Installing the CA on a device grants this local CA substantial trust. Treat the private key accordingly.

Do not install the CA on devices you do not own or administer, and do not intercept traffic without authorization.
