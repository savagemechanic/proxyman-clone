# Proxyman Clone

> **A free, open-source HTTPS debugging proxy for developers.**
>
> Inspect traffic. Debug APIs. Intercept TLS. Rewrite requests and responses. Own your tooling.

⭐ **If you believe essential developer tools should have a serious open-source alternative, star the repository.** It helps other developers discover the project and tells us where to invest our time.

---

## Why this project exists

Modern applications are distributed across mobile clients, browsers, APIs, microservices, third-party SDKs, and infrastructure you do not control. When something breaks, being able to see exactly what crossed the wire is extraordinarily useful.

[Proxyman](https://proxyman.com/) is an excellent commercial HTTP debugging application. It is proprietary software and some functionality is sold through paid licenses.

This repository is an **independent, community-built open-source project** inspired by the general category of developer tools that Proxyman, Charles Proxy, Fiddler, mitmproxy, and similar products occupy.

The goal is ambitious:

**Build a polished, native-feeling network debugging workstation that developers can inspect, modify, extend, and use without a paid license.**

This is not an official Proxyman product, is not affiliated with Proxyman LLC, and does not contain Proxyman source code or proprietary assets. "Proxyman" is a trademark of its respective owner.

---

## The vision

Most developers should not need several separate tools just to understand an application's network behavior.

The long-term experience should be simple:

1. Launch the application.
2. Configure your device or application to use the proxy.
3. Install/trust the local development CA when HTTPS interception is required.
4. Watch requests appear in real time.
5. Search, filter, inspect, replay, breakpoint, or rewrite them.
6. Export what matters and get back to building.

The interface should feel like a professional desktop application rather than a thin GUI wrapped around a terminal proxy.

---

## Planned capabilities

### HTTP / HTTPS capture

- HTTP/1.1 proxying
- HTTPS interception via locally generated CA certificates
- CONNECT tunneling
- HTTP/2 support
- WebSocket inspection
- Server-Sent Events
- Streaming request and response bodies
- Automatic certificate generation and caching
- Configurable SSL proxying rules
- Certificate and TLS metadata inspection

### Traffic inspector

- Live request timeline
- Request and response headers
- Query parameters
- Cookies
- Form data
- JSON, XML, HTML, text, binary and image previews
- Pretty printing and syntax highlighting
- Raw request/response view
- Timing information
- Status, method, MIME type and size metadata
- Request tree/grouping by host
- Search and advanced filtering

### Interception & modification

- Request breakpoints
- Response breakpoints
- Edit requests before forwarding
- Edit responses before delivery
- Local response overrides
- Map Local
- Map Remote
- URL rewrites
- Header rewrites
- Query/body rewrites
- Rule enable/disable controls
- Rule persistence

### Replay & API debugging

- Repeat requests
- Duplicate and edit captured requests
- Compose requests manually
- Request history
- cURL import/export
- Copy as cURL
- Compare requests and responses

### Developer workflow

- HAR import/export
- Session save/open
- Pin/favorite requests
- Comments/annotations
- Domain allow/block lists
- Ignore lists
- Multiple tabs/windows where useful
- Keyboard-first navigation
- Dark/light appearance

### Devices & environments

The project is intended to support debugging traffic from:

- macOS applications
- browsers
- iOS devices and simulators
- Android devices and emulators
- command-line applications
- backend services
- other devices capable of using an HTTP proxy

### Extensibility

A mature release should expose a programmable interception layer so developers can automate transformations and analysis rather than being limited to built-in rules.

Possible directions include:

- JavaScript-based scripting
- reusable rule collections
- plugins/extensions
- custom inspectors
- automation hooks

---

## Architecture

The project is being designed as two cooperating layers rather than putting networking logic directly into UI code.

```text
┌───────────────────────────────────────────────┐
│                Desktop Application            │
│                                               │
│  Traffic List   Inspector   Rules   Composer  │
│         │             │       │        │       │
└─────────┼─────────────┼───────┼────────┼───────┘
          │             Application API
┌─────────▼─────────────────────────────────────┐
│                  Proxy Core                   │
│                                               │
│  Listener ─► HTTP ─► TLS MITM ─► Upstream    │
│      │          │        │           │         │
│      └────► Capture / Rules / Events ◄────────┘
└───────────────────────────────────────────────┘
                         │
                         ▼
                     Internet
```

The separation matters. The proxy engine should eventually be usable independently of the graphical client and should remain testable without launching the UI.

---

## MVP

For this project, **TLS interception is part of the MVP**.

A network debugger that only handles plaintext HTTP is not useful enough for modern application development, where the overwhelming majority of interesting traffic is HTTPS.

The first genuinely usable milestone therefore targets:

- local HTTP proxy server
- CONNECT support
- local CA creation and management
- per-host leaf certificate generation
- HTTPS MITM for explicitly configured development clients
- live request/response capture
- desktop traffic list
- request/response inspector
- JSON/text/raw body viewers
- search and filtering
- request replay
- breakpoints
- request/response modification
- basic rewrite rules
- HAR export
- persistent settings

We would rather ship a smaller **real HTTPS debugger** than call a plaintext traffic viewer an MVP.

---

## Roadmap

### Phase 1 — Foundation

- repository/tooling structure
- proxy-core package
- desktop shell
- shared traffic models
- event pipeline
- logging and diagnostics
- automated tests

### Phase 2 — HTTP + TLS engine

- HTTP proxy listener
- CONNECT handling
- CA generation
- certificate persistence
- dynamic leaf certificates
- TLS interception
- upstream forwarding
- streaming capture

### Phase 3 — Inspector UI

- live traffic table
- sidebar/domain organization
- request metadata
- header/query/cookie inspectors
- body viewers
- syntax highlighting
- filtering/search
- timing display

### Phase 4 — Power tools

- breakpoints
- request editing
- response editing
- replay
- composer
- Map Local
- Map Remote
- rewrite engine

### Phase 5 — Protocol depth

- HTTP/2
- WebSockets
- SSE
- improved streaming
- richer TLS information

### Phase 6 — Workflow & polish

- HAR import/export
- saved sessions
- certificate onboarding
- device setup guides
- keyboard shortcuts
- performance work
- packaging and releases

### Phase 7 — Extensibility

- scripting API
- plugin architecture
- reusable community rules
- custom inspectors

---

## Security model

HTTPS interception is powerful and must be handled carefully.

The application creates a local Certificate Authority so that **devices you explicitly configure and authorize** can trust certificates generated by the proxy during development.

The project will follow several principles:

- private CA keys remain local
- no telemetry containing captured traffic
- no cloud dependency for interception
- explicit certificate installation/trust
- clear visibility when interception is active
- easy CA removal/reset
- sensible local-only defaults
- captured secrets are treated as sensitive data

Never install the development CA on a device you do not control or have permission to configure.

---

## Legal & ethical use

This software is intended for legitimate development, debugging, testing, interoperability, education, and authorized security research.

Only intercept traffic belonging to systems/devices you own or are explicitly authorized to test.

The existence of an interception feature does not grant permission to intercept another person's communications.

---

## Project status

🚧 **Early development**

The architecture and core implementation are being built now. Expect rapid changes before the first stable release.

If you found this repository because you want an open-source desktop HTTP/HTTPS debugger, **star it now and watch the project grow.**

Stars are more than vanity for an early open-source project: they make the project easier to discover, help attract contributors, and give us a useful signal that developers want this tool to exist.

---

## Contributing

Contributors are welcome.

You can help with:

- proxy/networking internals
- TLS and certificate handling
- protocol implementations
- desktop UI/UX
- performance profiling
- tests
- documentation
- platform packaging
- accessibility
- security review

Before starting a large feature, open an issue so implementation direction can be coordinated. See [CONTRIBUTING.md](CONTRIBUTING.md) for the development and review workflow.

If you cannot contribute code, starring the repository, testing releases, filing precise bug reports, and sharing the project are all valuable contributions.

---

## What we will not do

Being inspired by a product category does not mean copying proprietary implementation details.

This project will not:

- use leaked or reverse-engineered proprietary source code
- redistribute proprietary Proxyman assets
- pretend to be an official Proxyman release
- intentionally reproduce protected branding

Features will be implemented independently using public protocol specifications, platform APIs, original code, and established open-source techniques.

---

## Why open source?

A network debugging proxy occupies an unusually privileged position: it can see some of the most sensitive data a developer handles.

For that category of tool, source availability is especially valuable. Developers should be able to inspect how certificates are generated, where traffic is stored, what leaves their machine, and exactly how interception works.

Open source also means the tool can outlive a company, pricing model, product strategy, or individual maintainer.

No subscription gate is required to understand your own network traffic.

---

## Support the project

If this is a tool you want on your machine:

**⭐ Star the repository.**

Then consider watching releases, opening issues, contributing code, or sharing it with another developer who debugs APIs or mobile applications.

Every early star makes the project slightly easier for the next developer to discover.

---

## License

Proxyman Clone is released under the [MIT License](LICENSE). You may use, modify, distribute, and build on it subject to the license terms.

---

<p align="center">
  <strong>Inspect the wire. Understand the system. Own the tool.</strong>
</p>
