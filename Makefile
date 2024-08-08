FUNCTIONS := handler
LAMBDA_NAME := coralogix-aws-shipper
ARCH := $(or ${RUST_ARCH},arm64)
ARCH_SPLIT = $(subst -, ,$(ARCH))

# build-%:
# 	cargo lambda build --release --${ARCH}
# 	@mkdir -p $(ARTIFACTS_DIR)
# 	@cp -v ./target/lambda/${LAMBDA_NAME}/bootstrap $(ARTIFACTS_DIR)
# 	set +xv

build-LambdaFunction:
	cargo lambda build --release --locked --target aarch64-unknown-linux-gnu.2.17
	cp ./target/lambda/coralogix-aws-shipper/bootstrap $(ARTIFACTS_DIR)

delete:
	sam delete