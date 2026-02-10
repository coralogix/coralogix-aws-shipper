use crate::metrics::config::Config;
// use aws_lambda_events::encodings::Base64Data;
use aws_lambda_events::event::firehose::{
    KinesisFirehoseEvent, KinesisFirehoseResponse, KinesisFirehoseResponseRecord,
    KinesisFirehoseResponseRecordMetadata,
};
use lambda_runtime::Error;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value;
use opentelemetry_proto::tonic::common::v1::AnyValue;
use opentelemetry_proto::tonic::common::v1::KeyValue;
use prost::encoding::{decode_varint, encode_varint};
use prost::Message;
use reqwest;
use std::time::Instant;
use tracing::{debug, error, info};

#[derive(Debug, Default)]
struct MetricsBatch {
    request: ExportMetricsServiceRequest,
    encoded_size: usize,
}

impl MetricsBatch {
    fn is_empty(&self) -> bool {
        self.request.resource_metrics.is_empty()
    }

    fn extend(&mut self, message: ExportMetricsServiceRequest, encoded_len: usize) {
        self.request
            .resource_metrics
            .extend(message.resource_metrics.into_iter());
        self.encoded_size += encoded_len;
    }

    fn clear(&mut self) {
        self.request = ExportMetricsServiceRequest::default();
        self.encoded_size = 0;
    }
}

fn split_length_delimited(data: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut chunks = Vec::new();
    let mut remaining = data;

    while remaining.len() > 0 {
        // Decode the length prefix (varint)
        let length = decode_varint(&mut remaining)
            .map_err(|e| format!("Failed to decode varint: {}", e))? as usize;

        // Ensure the remaining data is enough for the length
        if length > remaining.len() {
            return Err("Insufficient data for length-delimited field".to_string());
        }

        // Extract the length-delimited chunk
        let chunk = &remaining[0..length];
        chunks.push(chunk);

        // Move the position forward
        remaining = &remaining[length..];
    }

    Ok(chunks)
}

// TODO: look into whether these messages can be batched instead of sending one at a time
#[allow(dead_code)]
fn re_batch(chunks: Vec<Vec<u8>>) -> Vec<u8> {
    let mut batched_data = Vec::new();
    for chunk in chunks {
        let length = chunk.len() as u64;
        let mut buf = Vec::new();
        encode_varint(length, &mut buf);
        batched_data.extend_from_slice(&buf);
        batched_data.extend_from_slice(&chunk);
    }
    batched_data
}

/// handle_mesage - decodes messages, updates datapoints/attributes with cx_application_name and cx_subsystem_name
fn transform_message(
    message: &[u8],
    app_name: &str,
    subsystem_name: &str,
) -> Result<ExportMetricsServiceRequest, Error> {
    let mut decoded_message = ExportMetricsServiceRequest::decode(&*message)?;
    debug!("decoded metrics: {:?}", decoded_message);

    for resource in decoded_message.resource_metrics.iter_mut() {
        for scope_metrics in resource.scope_metrics.iter_mut() {
            //debug!("Scope Metrics Iter: {:?}", scope_metrics.metrics.iter());
            for metric in scope_metrics.metrics.iter_mut() {
                //metric.unit = curly_braces_re.replace_all(&metric.unit, "").to_string().to_lowercase();
                metric.unit = "".to_string();
                debug!("Metric Metadata: {:?}", metric.data);
                if let Some(data) = &mut metric.data {
                    match data {
                        opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => debug!("Gauge: {:?}", gauge),
                        opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => debug!("Sum: {:?}", sum),
                        opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(histogram) => debug!("Histogram: {:?}", histogram),
                        opentelemetry_proto::tonic::metrics::v1::metric::Data::ExponentialHistogram(exponential_histogram) => debug!("ExponentialHistogram: {:?}", exponential_histogram),

                        opentelemetry_proto::tonic::metrics::v1::metric::Data::Summary(summary) => {
                            debug!("Summary: {:?}", summary);

                            // Iterate over mutable references to data_points to modify them directly
                            summary.data_points.iter_mut().for_each(|data_point| {
                                let mut new_attributes = Vec::new();

                                // Collect attributes if the key is "Dimensions"
                                data_point.attributes.iter().for_each(|label| {
                                    if label.key == "Dimensions" {
                                        debug!("DimensionsLabels: {:?}", label);
                                        if let Some(AnyValue { value: Some(any_value::Value::KvlistValue(kv_list)) }) = &label.value {
                                            for kv in &kv_list.values {
                                                new_attributes.push(kv.clone());
                                            }
                                        }
                                    }
                                });
                                // Add cx.application.name and cx.subsystem.name attributes
                                new_attributes.push(KeyValue {
                                    key: "cx.application.name".to_string(),
                                    value: Some(AnyValue {
                                        value: Some(any_value::Value::StringValue(app_name.to_string())),
                                    }),
                                });
                                new_attributes.push(KeyValue {
                                    key: "cx.subsystem.name".to_string(),
                                    value: Some(AnyValue {
                                        value: Some(any_value::Value::StringValue(subsystem_name.to_string())),
                                    }),

                                });

                                debug!("Data Point Attributes: {:?}", new_attributes);
                                data_point.attributes = data_point
                                    .attributes
                                    .iter()
                                    .filter(|label| label.key != "Dimensions")
                                    .cloned()
                                    .collect();
                                data_point.attributes.extend(new_attributes.iter().cloned());
                                // Extend data_point attributes with the new attributes
                                //data_point.attributes.extend(new_attributes.iter().cloned());
                                debug!("Final DataPoint: {:?}", data_point);
                            });

                            // Print the metric after all modifications
                            debug!("Final Metric Details: {:?}", metric);
                        }
                    }
                }
            }
        }
    }

    Ok(decoded_message)
}

/// coralogix_send - sends metrics to Coralogix
async fn coralogix_send(config: &Config, message: Vec<u8>) -> Result<(), Error> {
    let uri = format!("{}/v1/metrics", config.endpoint);

    debug!("sending to metric data to uri: {:?}", uri);

    let start = Instant::now();
    let bytes = message.len();
    let response = reqwest::Client::new()
        .post(&uri)
        .header("Content-Type", "application/x-protobuf")
        .header(
            "Authorization",
            &format!("Bearer {}", config.api_key.token()),
        )
        .body(message)
        .send()
        .await?;

    info!(
        status = %response.status(),
        bytes,
        elapsed_ms = start.elapsed().as_millis(),
        uri = %uri,
        "metrics HTTP request completed"
    );

    Ok(())
}

/// encode_request - encodes an ExportMetricsServiceRequest to bytes
fn encode_request(request: &ExportMetricsServiceRequest) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();
    request.encode(&mut buf).map_err(|e| {
        let err = format!("failed to encode request: {}", e);
        error!("{}", err);
        err
    })?;
    Ok(buf)
}

/// flush_batch - sends the current batch to Coralogix
async fn flush_batch(config: &Config, batch: &mut MetricsBatch) -> Result<(), Error> {
    if batch.is_empty() {
        return Ok(());
    }

    let body = encode_request(&batch.request)?;
    info!(
        bytes = body.len(),
        tracked_bytes = batch.encoded_size,
        max_bytes = config.batch_max_size_bytes,
        resource_metrics = batch.request.resource_metrics.len(),
        "flushing aggregated metrics due to batch size threshold"
    );
    coralogix_send(config, body).await.map_err(|e| {
        let err = format!("failed to send aggregated metric data to coralogix: {}", e);
        error!("{}", err);
        err
    })?;
    batch.clear();
    Ok(())
}

/// try_add_to_batch - attempts to add a transformed message to the batch, flushing if needed
async fn try_add_to_batch(
    config: &Config,
    aggregated_opt: &mut Option<MetricsBatch>,
    transformed_message: ExportMetricsServiceRequest,
    message_body: Vec<u8>,
) -> Result<(), Error> {
    let message_size = message_body.len();

    if message_size > config.batch_max_size_bytes {
        if let Some(batch) = aggregated_opt.as_mut() {
            if !batch.is_empty() {
                flush_batch(config, batch).await?;
            }
        }

        info!(
            bytes = message_size,
            max_bytes = config.batch_max_size_bytes,
            "single transformed message exceeds batch size; sending as-is"
        );
        coralogix_send(config, message_body).await.map_err(|e| {
            let err = format!(
                "failed to send oversize single metric payload to coralogix: {}",
                e
            );
            error!("{}", err);
            err
        })?;
        return Ok(());
    }

    let batch = aggregated_opt.get_or_insert_with(MetricsBatch::default);

    if !batch.is_empty() && batch.encoded_size + message_size > config.batch_max_size_bytes {
        flush_batch(config, batch).await?;
    }

    let before_rm = batch.request.resource_metrics.len();
    batch.extend(transformed_message, message_size);
    let after_rm = batch.request.resource_metrics.len();
    debug!(
        added_resource_metrics = (after_rm.saturating_sub(before_rm)),
        total_resource_metrics = after_rm,
        tracked_bytes = batch.encoded_size,
       "batched metrics aggregated"
    );

    Ok(())
}

/// process_messages_with_batching - processes messages in batching mode
async fn process_messages_with_batching(
    config: &Config,
    messages: Vec<&[u8]>,
    aggregated_opt: &mut Option<MetricsBatch>,
) -> Result<(), Error> {
    for message in messages {
        let transformed_message = transform_message(message, &config.app_name, &config.sub_name)
            .map_err(|e| {
                let err = format!("failed to transform message: {}", e);
                error!("{}", err);
                err
            })?;

        let message_body = encode_request(&transformed_message)?;
        try_add_to_batch(config, aggregated_opt, transformed_message, message_body).await?;
   }
    Ok(())
}

/// process_messages_without_batching - processes messages without batching (send individually)
async fn process_messages_without_batching(
    config: &Config,
    messages: Vec<&[u8]>,
    record_index: usize,
) -> Result<(), Error> {
    info!(
        per_record_messages = messages.len(),
        "batching disabled; sending individually"
    );

    for (midx, message) in messages.into_iter().enumerate() {
        let transformed_message = transform_message(message, &config.app_name, &config.sub_name)
            .map_err(|e| {
                let err = format!("failed to transform message: {}", e);
                error!("{}", err);
                err
            })?;

        let body = encode_request(&transformed_message)?;
        debug!(
            record_index = record_index,
            message_index = midx,
            bytes = body.len(),
            "sending single metrics payload"
        );

        coralogix_send(config, body).await.map_err(|e| {
            let err = format!("failed to send metric data to coralogix: {}", e);
            error!("{}", err);
            err
        })?;
    }
    Ok(())
}

/// send_final_batch - sends the remaining aggregated batch if any
async fn send_final_batch(
    config: &Config,
    aggregated_opt: &mut Option<MetricsBatch>,
    total_records: usize,
    total_messages_seen: usize,
) -> Result<(), Error> {
    if let Some(batch) = aggregated_opt.as_mut() {
        if batch.is_empty() {
            info!("batching enabled but no aggregated resource_metrics to send");
            return Ok(());
        }

        // Compute some insight into the aggregated shape
        let total_resource_metrics = batch.request.resource_metrics.len();
        let mut total_scope_metrics = 0usize;
        let mut total_metrics = 0usize;
        for rm in &batch.request.resource_metrics {
            total_scope_metrics += rm.scope_metrics.len();
            for sm in &rm.scope_metrics {
                total_metrics += sm.metrics.len();
            }
        }

        let body = encode_request(&batch.request)?;

        info!(
            total_records,
            total_messages_seen,
            total_resource_metrics,
            total_scope_metrics,
            total_metrics,
            bytes = body.len(),
            tracked_bytes = batch.encoded_size,
            "sending aggregated metrics payload"
        );

        coralogix_send(config, body).await.map_err(|e| {
            let err = format!("failed to send aggregated metric data to coralogix: {}", e);
            error!("{}", err);
            err
        })?;

        batch.clear();

        return Ok(());
    }

    info!("batching enabled but aggregator not initialized");
   Ok(())
}

// transform_firehose_event - processes the KinesisFirehoseEvent and sends the transformed data to Coralogix
pub async fn transform_firehose_event(
    config: &Config,
    event: KinesisFirehoseEvent,
) -> Result<KinesisFirehoseResponse, Error> {
    // Log start
    info!(
        batching_enabled = config.batching_enabled,
        total_records = event.records.len(),
        endpoint = %config.endpoint,
        app = %config.app_name,
        subsystem = %config.sub_name,
        "metrics transform start"
    );

    // Initialize batch aggregator if batching is enabled
    let mut aggregated_opt: Option<MetricsBatch> = if config.batching_enabled {
        Some(MetricsBatch::default())
   } else {
        None
    };

    let mut total_messages_seen: usize = 0;
    let mut results = Vec::new();

    // Process each record
    for (idx, record) in event.records.clone().into_iter().enumerate() {
        let otel_payload = record.data.clone();

       // Split length-delimited messages
        let messages = split_length_delimited(&otel_payload.0).map_err(|e| {
            let err = format!("failed to split length-delimited data: {}", e);
            error!("{}", err);
            err
        })?;

       debug!(
            record_index = idx,
            message_count = messages.len(),
            "record parsed"
        );
        total_messages_seen += messages.len();

        // Route to appropriate processor based on batching mode
        if config.batching_enabled {
            process_messages_with_batching(config, messages, &mut aggregated_opt).await?;
        } else {
            process_messages_without_batching(config, messages, idx).await?;
        }

        let mut response_record: KinesisFirehoseResponseRecord = KinesisFirehoseResponseRecord::default();
        let mut metadata: KinesisFirehoseResponseRecordMetadata = KinesisFirehoseResponseRecordMetadata::default();
        metadata.partition_keys = std::collections::HashMap::new();
        response_record.metadata = metadata;
        response_record.record_id = record.record_id;
        response_record.result = Some("Dropped".to_string());
        response_record.data = record.data;
        results.push(response_record);
    }

    // Send final batch if batching is enabled
    if config.batching_enabled {
        send_final_batch(
            config,
            &mut aggregated_opt,
            event.records.len(),
            total_messages_seen,
        )
        .await?;
   }

    // Return response to Firehose
    let mut response: KinesisFirehoseResponse = KinesisFirehoseResponse::default();
    response.records = results;
    Ok(response)

}
