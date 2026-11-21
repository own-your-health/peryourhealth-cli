BIN := peryourhealth
CARGO ?= cargo

.PHONY: build release test lint fmt fmt-check clean install deps smoke verify dev

# Every target that produces a binary signs it (cli-common's dev-sign.sh), so
# the code identity stays stable between builds. peryourhealth keeps one keychain
# item, the vault's key, and an ad-hoc signature would prompt for it on every
# rebuild.
build: SIGN_TARGET = target/debug/$(BIN)
build:
	$(CARGO) build
	@$(SIGN)

release: SIGN_TARGET = target/release/$(BIN)
release:
	$(CARGO) build --release
	@$(SIGN)

test:
	$(CARGO) test

lint:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

clean:
	$(CARGO) clean

deps:
	$(CARGO) fetch

# `--locked` builds from Cargo.lock, the versions the tests and audit ran on.
install: SIGN_TARGET = $${CARGO_INSTALL_ROOT:-$$HOME/.cargo}/bin/$(BIN)
install:
	$(CARGO) install --path . --locked --force
	@$(SIGN)

dev: SIGN_TARGET = target/debug/$(BIN)
dev:
	$(CARGO) build
	@$(SIGN)

# Offline smoke test: no network, and never the real keychain, vault or
# browser profile: PERYOURHEALTH_KEYCHAIN_SERVICE names a service that holds
# nothing, with the vault under target/smoke-config, and peryourhealth leaves the
# kept browser profiles alone. Nothing is stored, so no key is read.
HERMETIC := PERYOURHEALTH_KEYCHAIN_SERVICE=piekstra.peryourhealth.hermetic-tests XDG_DATA_HOME=$(CURDIR)/target/smoke-data \
	XDG_CONFIG_HOME=$(CURDIR)/target/smoke-config
smoke: build
	@./target/debug/$(BIN) --version >/dev/null
	@env -u PERYOURHEALTH_PROFILE ./target/debug/$(BIN) info >/dev/null
	@env -u PERYOURHEALTH_PROFILE ./target/debug/$(BIN) completions bash >/dev/null
	@rm -rf target/smoke-config
	@$(HERMETIC) ./target/debug/$(BIN) -q profiles add smoke >/dev/null
	@$(HERMETIC) ./target/debug/$(BIN) -q auth status --profile smoke --json >/dev/null
	@$(HERMETIC) ./target/debug/$(BIN) -q api GET /Full/Services --profile smoke --json >/dev/null 2>&1; \
		test $$? -eq 3 || { echo "smoke: api without a session did not exit 3"; exit 1; }
	@for c in "auth login" "auth status" "auth logout" "auth reset" "profiles list" "profiles add" \
		"profiles remove" "config path" "config show" api info; do \
		./target/debug/$(BIN) $$c --help >/dev/null || exit 1; done
	@echo "smoke ok"

verify: fmt-check lint test smoke

define SIGN
if [ -x "$$HOME/Dev/cli-common/scripts/dev-sign.sh" ]; then \
	"$$HOME/Dev/cli-common/scripts/dev-sign.sh" "$(SIGN_TARGET)"; \
else echo "cli-common/scripts/dev-sign.sh not found — $(SIGN_TARGET) left ad-hoc signed"; fi
endef
