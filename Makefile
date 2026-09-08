.PHONY: rust-check rust-test rust-fmt swift-build swift-test check

rust-check:
	cd rust && cargo check --workspace

rust-test:
	cd rust && cargo test --workspace

rust-fmt:
	cd rust && cargo fmt --all -- --check

swift-build:
	cd macos && swift build

swift-test:
	cd macos && swift test

check: rust-fmt rust-check rust-test swift-build swift-test
