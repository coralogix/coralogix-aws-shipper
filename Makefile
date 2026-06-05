FUNCTIONS := handler
LAMBDA_NAME := coralogix-aws-shipper
ARCH := $(or ${RUST_ARCH},arm64)

# CARGO_LAMBDA_COMPILER is opt-in only.
# Leave unset for the default build (cargo-lambda picks zigbuild internally
# when the .2.17 glibc-pinned target is used, and lets cmake use the native
# cross-compiler for C crates like aws-lc-fips-sys).
# Set to "cargo" only for FIPS builds that require the native cargo backend:
#   CARGO_LAMBDA_COMPILER=cargo make build-LambdaFunction
LAMBDA_COMPILER := ${CARGO_LAMBDA_COMPILER}

# When the native cargo compiler is requested, set up the cross-compile env
# and drop the .2.17 glibc pin (zig is not involved).
ifeq ($(ARCH),arm64)
    ifeq ($(LAMBDA_COMPILER),cargo)
        TARGET_ARCH := aarch64-unknown-linux-gnu
        AARCH64_BUILD_ENV := \
            CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=$(or ${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER},aarch64-linux-gnu-gcc) \
            TARGET_CC_aarch64_unknown_linux_gnu=$(or ${TARGET_CC_aarch64_unknown_linux_gnu},clang) \
            TARGET_CXX_aarch64_unknown_linux_gnu=$(or ${TARGET_CXX_aarch64_unknown_linux_gnu},clang++) \
            TARGET_CFLAGS_aarch64_unknown_linux_gnu="$(or ${TARGET_CFLAGS_aarch64_unknown_linux_gnu},--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu)" \
            TARGET_CXXFLAGS_aarch64_unknown_linux_gnu="$(or ${TARGET_CXXFLAGS_aarch64_unknown_linux_gnu},--target=aarch64-linux-gnu --sysroot=/usr/aarch64-linux-gnu)" \
            AR_aarch64_unknown_linux_gnu=$(or ${AR_aarch64_unknown_linux_gnu},llvm-ar)
    else
        TARGET_ARCH := aarch64-unknown-linux-gnu.2.17
    endif
else ifeq ($(ARCH),x86-64)
    ifeq ($(LAMBDA_COMPILER),cargo)
        TARGET_ARCH := x86_64-unknown-linux-gnu
    else
        TARGET_ARCH := x86_64-unknown-linux-gnu.2.17
    endif
else
    $(error Unsupported architecture: $(ARCH))
endif

# Pass --compiler only when LAMBDA_COMPILER is explicitly set.
COMPILER_FLAG := $(if $(LAMBDA_COMPILER),--compiler $(LAMBDA_COMPILER),)

build-LambdaFunction:
	$(AARCH64_BUILD_ENV) cargo lambda build $(COMPILER_FLAG) --release --target $(TARGET_ARCH)
	@mkdir -p $(ARTIFACTS_DIR)
	cp ./target/lambda/$(LAMBDA_NAME)/bootstrap $(ARTIFACTS_DIR)

delete:
	sam delete

# ---------------------------------------------------------------------------
# pre-merge-check: mirrors what CI does on push to master, without S3/SAR.
#
# Usage:
#   make pre-merge-check             # arm64 build (default)
#   make pre-merge-check RUST_ARCH=x86-64
#
# Steps (in order):
#   1. cfn-lint      – same check as the tests.yaml PR workflow
#   2. sam validate  – same check as the publish.yaml validate job
#   3. cargo test    – unit + integration tests
#   4. cargo clippy  – deny warnings (catches issues before the build)
#   5. sam build     – actual cross-compilation (the most common failure point)
# ---------------------------------------------------------------------------
.PHONY: pre-merge-check
pre-merge-check:
	@echo "=== [1/5] cfn-lint ==="
	cfn-lint --template template.yaml --region us-east-1 --config-file .cfn-lint.yaml
	@echo "=== [2/5] sam validate ==="
	sam validate
	@echo "=== [3/5] cargo test ==="
	cargo test --all -- --nocapture
	@echo "=== [4/5] cargo clippy ==="
	cargo clippy --all-targets -- -D warnings
	@echo "=== [5/5] sam build (ARCH=$(ARCH)) ==="
	RUST_ARCH=$(ARCH) CARGO_LAMBDA_COMPILER=$(LAMBDA_COMPILER) sam build
	@echo ""
	@echo "All checks passed. Safe to merge."
