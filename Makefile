FUNCTIONS := handler
LAMBDA_NAME := coralogix-aws-shipper
ARCH := arm64
ARCH_SPLIT = $(subst -, ,$(ARCH))

build-%:
	cargo lambda build --release --${ARCH}
	@mkdir -p .aws-sam/build/$*
	@cp -v ./target/lambda/${LAMBDA_NAME}/bootstrap $(ARTIFACTS_DIR)

delete:
	sam delete

