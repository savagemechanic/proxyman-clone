# Getting started

Proxyman Clone is currently a **development preview**. The proxy engine and native macOS client are functional, but the project is not packaged as a signed `.app` release yet.

This guide gets a contributor from a fresh checkout to a captured HTTPS request without adding the development CA to the macOS trust store.

## Prerequisites

- macOS 14 or newer
- Git
- a stable Rust toolchain with Cargo
- Xcode / Xcode Command Line Tools with Swift 6 support
- `curl` for the verification request

Verify the toolchains:

```bash
rustc --version
cargo --version
swift --version
```

## 1. Clone the repository

```bash
git clone https://github.com/savagemechanic/proxyman-clone.git
cd proxyman-clone
```

## 2. Start the proxy engine with HTTPS interception

In terminal 1:

```bash
cd rust
PROXYMAN_CLONE_TLS_INTERCEPT=1 cargo run -p proxy-daemon
```

By default the daemon exposes only loopback listeners:

- HTTP proxy: `127.0.0.1:8080`
- app control socket: `127.0.0.1:9099`

On first TLS-enabled launch it creates a local development CA at:

```text
~/Library/Application Support/ProxymanClone/certificates/ca-cert.pem
```

Its private key is stored beside it as `ca-key.pem`. **Do not share, upload, or commit that private key.** Delete the certificate directory if you want the project to generate a fresh development CA.

## 3. Launch the native traffic inspector

From the repository root, in terminal 2:

```bash
cd macos
swift run ProxymanCloneApp
```

The app connects to the daemon's loopback control socket and refreshes captured traffic automatically.

## 4. Verify HTTPS interception safely

In terminal 3, make one HTTPS request through the proxy while telling only this `curl` process to trust the generated CA:

```bash
curl \
  --proxy http://127.0.0.1:8080 \
  --cacert "$HOME/Library/Application Support/ProxymanClone/certificates/ca-cert.pem" \
  https://example.com/
```

The request should appear in the Traffic workspace with `https` as its scheme. Select it to inspect request/response headers and captured body previews.

This method does **not** globally trust the development CA. It is the recommended first verification path.

## Running without TLS interception

Start the daemon without `PROXYMAN_CLONE_TLS_INTERCEPT`:

```bash
cd rust
cargo run -p proxy-daemon
```

Plain HTTP traffic is inspected normally. HTTPS `CONNECT` traffic is tunneled transparently, so the daemon can record the tunnel but cannot inspect the encrypted HTTP request/response inside it.

## Local data

The current default application-support directory is:

```text
~/Library/Application Support/ProxymanClone/
```

It contains:

- `certificates/ca-cert.pem` — local development CA certificate
- `certificates/ca-key.pem` — sensitive local development CA private key
- `rewrite-rules.json` — versioned persisted rewrite-rule configuration

Captured traffic is currently held in the daemon's bounded in-memory session store; it is not automatically uploaded anywhere.

Development overrides:

```text
PROXYMAN_CLONE_PROXY_ADDR
PROXYMAN_CLONE_CONTROL_ADDR
PROXYMAN_CLONE_CERT_DIR
PROXYMAN_CLONE_RULES_PATH
PROXYMAN_CLONE_TLS_INTERCEPT
```

Keep the control listener on loopback unless you are deliberately developing a secured remote-control design. The current control protocol assumes a local trust boundary.

## Verify the repository

Run the same core checks used by CI:

```bash
make check
```

That runs Rust formatting, workspace compilation/tests, and Swift build/tests.

You can also run them individually:

```bash
make rust-fmt
make rust-check
make rust-test
make swift-build
make swift-test
```

## What works today

The development preview already includes:

- HTTP proxying and CONNECT tunneling
- opt-in HTTPS MITM with a locally generated CA
- bounded request/response capture
- native SwiftUI traffic list and inspector
- search and status/protocol filtering
- JSON/text body previews
- request and response rewrite rules
- persisted rewrite-rule configuration
- native rule editor
- Copy as cURL
- HAR 1.2 export
- safe one-click request replay

The README roadmap intentionally includes features that are still under active development, such as the editable Composer, interactive breakpoints, Map Local/Remote, deeper protocol support, packaging, and extension APIs.

## Troubleshooting

### `Address already in use`

Another process is using port `8080` or `9099`. Stop it, or choose development addresses explicitly:

```bash
PROXYMAN_CLONE_PROXY_ADDR=127.0.0.1:18080 \
PROXYMAN_CLONE_CONTROL_ADDR=127.0.0.1:19099 \
PROXYMAN_CLONE_TLS_INTERCEPT=1 \
cargo run -p proxy-daemon
```

The current Swift client defaults to control port `9099`, so changing the control address is mainly useful for engine development until configurable app settings land.

### HTTPS request fails certificate verification

Confirm the daemon was started with `PROXYMAN_CLONE_TLS_INTERCEPT=1` and that the CA file exists. For the first test, use `curl --cacert` exactly as shown above rather than globally trusting the CA.

### HTTPS works but only a tunnel appears

TLS interception is disabled. Restart the daemon with `PROXYMAN_CLONE_TLS_INTERCEPT=1` and retry with a client that trusts the generated CA.

### The app says `Engine unavailable`

Make sure `proxy-daemon` is running and listening on the default control address `127.0.0.1:9099`.

## Removing the development CA

If you only used `curl --cacert`, nothing was added to the macOS trust store. To remove the project's locally generated CA/key files, stop the daemon and delete:

```text
~/Library/Application Support/ProxymanClone/certificates/
```

A new CA will be generated the next time TLS interception starts.
