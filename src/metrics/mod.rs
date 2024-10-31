
use crate::clients::AwsClients;
use async_recursion::async_recursion;
use lambda_runtime::{Error, LambdaEvent};
use crate::events;
use tracing::warn;

#[async_recursion]
// metric telemetry handler
// TODO: implement
pub async fn handler(
    _: &AwsClients,
    _: LambdaEvent<events::Combined>,
) -> Result<(), Error> {
    warn!("metrics telemetry mode not implemented");
    Ok(())
}
