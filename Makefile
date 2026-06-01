FUNCTIONS := handler
LAMBDA_NAME := coralogix-aws-shipper
ARCH := $(or ${RUST_ARCH},arm64)
LAMBDA_COMPILER := $(or ${CARGO_LAMBDA_COMPILER},cargo)

# Determine the target based on the architecture
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

build-LambdaFunction:
	$(AARCH64_BUILD_ENV) cargo lambda build --compiler $(LAMBDA_COMPILER) --release --target $(TARGET_ARCH)
	@mkdir -p $(ARTIFACTS_DIR)
	cp ./target/lambda/$(LAMBDA_NAME)/bootstrap $(ARTIFACTS_DIR)

delete:
	sam delete
