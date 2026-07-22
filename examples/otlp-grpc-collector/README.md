# OTLP/gRPC Collector enrichment example

This example receives logs from the Coralogix AWS Shipper over OTLP/gRPC,
adds `gateway.enriched=true` to every log record, and writes the enriched
payload to the Collector debug exporter.

## Start the Collector

Run an OpenTelemetry Collector Contrib image because the configuration uses
the transform processor:

```bash
docker run --rm \
  -p 4317:4317 \
  -v "$PWD/examples/otlp-grpc-collector/collector.yaml:/etc/otelcol-contrib/config.yaml:ro" \
  otel/opentelemetry-collector-contrib:latest
```

Pin the image to an approved version or digest for a production deployment.
The included receiver is plaintext. Keep it on a private network; do not expose
this unauthenticated listener to the public internet. For TLS, terminate HTTPS
on a private listener with a certificate that chains to a bundled public WebPKI
root. Private or custom CA roots are not currently supported. Forward OTLP/gRPC
to this receiver, or configure TLS on the Collector receiver.

## Deploy the shipper

Deploy the Lambda with these relevant stack parameters:

```text
LogExportProtocol=otlp_grpc
OTLPEndpoint=http://<collector-private-host>:4317
ApiKey=
LambdaSubnetID=<private-subnet>
LambdaSecurityGroupID=<collector-egress-security-group>
```

Use `http://<collector-private-host>:4317` for the included plaintext receiver.
For a TLS-enabled listener, use its `https://` origin with a certificate that
chains to a bundled public WebPKI root.
The endpoint must be an origin without a path, query string, or URI userinfo.
Values such as `http://user:password@collector-private-host:4317` are rejected;
configure Collector authentication outside the endpoint URI.

Place the Lambda in subnets that can route to the Collector and configure its
security group to allow egress to the listener. Allow corresponding ingress on
the Collector security group.

## Verify

1. Trigger any enabled AWS log integration.
2. In the Collector debug output, find the exported log record and verify that
   its attributes include `gateway.enriched: true`.
3. Inspect the Lambda configuration and verify that `CORALOGIX_API_KEY` is
   absent. Collector mode does not add authorization metadata.

The shipper does not automatically fall back to another destination if the
Collector is unavailable.

## Roll back

Update the stack to select `LogExportProtocol=coralogix_rest`, clear
`OTLPEndpoint`, and restore the Coralogix REST credentials (`ApiKey` and the
appropriate `CoralogixRegion` or `CustomDomain`). Verify direct REST delivery
before removing the Collector networking.
