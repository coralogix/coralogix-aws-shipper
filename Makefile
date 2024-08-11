FUNCTIONS := handler
LAMBDA_NAME := coralogix-aws-shipper
ARCH := $(or ${RUST_ARCH},arm64)

# Determine the target based on the architecture
ifeq ($(ARCH),arm64)
    TARGET_ARCH := aarch64-unknown-linux-gnu.2.17
else ifeq ($(ARCH),x86-64)
    TARGET_ARCH := x86_64-unknown-linux-gnu.2.17
else
    $(error Unsupported architecture: $(ARCH))
endif

build-LambdaFunction:
	cargo lambda build --release --target $(TARGET_ARCH)
	@mkdir -p $(ARTIFACTS_DIR)
	cp ./target/lambda/$(LAMBDA_NAME)/bootstrap $(ARTIFACTS_DIR)

delete:
	sam delete
