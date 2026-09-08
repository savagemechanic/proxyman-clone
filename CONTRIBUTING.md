# Contributing to Proxyman Clone

Thanks for helping build a serious open-source HTTP/HTTPS debugging proxy.

## Ground rules

This project is an independent clean-room implementation. Do not contribute proprietary Proxyman source code, assets, reverse-engineered private implementation details, leaked materials, or code copied from incompatible licenses.

Only test interception features against systems and devices you own or are explicitly authorized to inspect.

## Development setup

Requirements:

- Rust stable toolchain
- Xcode / Swift 6 toolchain on macOS
- Git

Useful commands:

```bash
make rust-check
make rust-test
make swift-build
make swift-test
make check
```

## Workflow

1. Pick or open an issue before beginning substantial work.
2. Create a focused branch from `main`.
3. Keep one conceptual change per PR.
4. Add or update tests for behavior changes.
5. Run formatting, linting, builds, and tests locally when possible.
6. Open a PR that links the issue and clearly states scope boundaries.
7. Address CI/review findings before merge.

## Commit conventions

Prefer Conventional Commit-style prefixes:

- `feat:` new behavior
- `fix:` bug fix
- `refactor:` internal restructuring
- `test:` tests only
- `docs:` documentation
- `ci:` automation
- `chore:` repository maintenance

## Networking and security changes

For proxy, TLS, certificate, parser, rule-engine, or persistence changes, PRs should explicitly document:

- trust/security boundary changes
- memory/body-size limits
- behavior on malformed input
- whether captured secrets can be persisted or logged
- backwards compatibility of the engine protocol

Never log CA private keys, authorization headers, cookies, captured bodies, or other sensitive traffic by default.

## Compatibility policy

The control protocol is versioned. Breaking protocol changes require an explicit protocol-version decision rather than silently changing serialized fields.

## Pull request quality bar

A PR is ready to merge when:

- CI passes
- the diff is focused and reviewable
- tests cover important behavior
- known limitations are documented
- security-sensitive defaults remain conservative
- user-facing changes have usable states and error handling

## Releases

`main` is the integration branch. Stable releases will be tagged using semantic versioning once the first public release line is established.
