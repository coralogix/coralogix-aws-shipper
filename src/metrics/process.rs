use crate::metrics::config::Config;
// use aws_lambda_events::encodings::Base64Data;
use aws_lambda_events::event::firehose::{KinesisFirehoseResponse, KinesisFirehoseEvent, KinesisFirehoseResponseRecord, KinesisFirehoseResponseRecordMetadata};
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
fn transform_message(message: &[u8], app_name: &str, subsystem_name: &str) -> Result<ExportMetricsServiceRequest, Error> {
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
    };

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

// transform_firehose_event - processes the KinesisFirehoseEvent and sends the transformed data to Coralogix
pub async fn transform_firehose_event(
    config: &Config,
    event: KinesisFirehoseEvent,
) -> Result<KinesisFirehoseResponse, Error> {
    // Iterate over all records in the KinesisFirehoseEvent
    let mut results = Vec::new();

    // High-level batching diagnostics
    info!(
        batching_enabled = config.batching_enabled,
        total_records = event.records.len(),
        endpoint = %config.endpoint,
        app = %config.app_name,
        subsystem = %config.sub_name,
        "metrics transform start"
    );
    // When batching is enabled, aggregate all messages across the entire event
    let mut aggregated_opt: Option<ExportMetricsServiceRequest> = if config.batching_enabled {
        Some(ExportMetricsServiceRequest { ..Default::default() })
    } else {
        None
    };
    let mut total_messages_seen: usize = 0;
    for (idx, record) in event.records.clone().into_iter().enumerate() {
        let otel_payload = record.data.clone();
        let messages = split_length_delimited(&otel_payload.0)
            .map_err(|e| {
                let err = format!("failed to split length-delimited data: {}", e);
                error!("{}", err);
                err
            })?;
        debug!(record_index = idx, message_count = messages.len(), "record parsed");
        total_messages_seen += messages.len();

        if config.batching_enabled {
            // Aggregate messages across all records; send once after the loop
            for message in messages {
                let transformed_message = transform_message(
                    message,
                    &config.app_name,
                    &config.sub_name,
                )
                .map_err(|e| {
                    let err = format!("failed to transform message: {}", e);
                    error!("{}", err);
                    err
                })?;

                // Initialize aggregator lazily
                let aggregated = aggregated_opt.get_or_insert_with(|| ExportMetricsServiceRequest { ..Default::default() });

                // Compute prospective size if we were to add this message to the current batch
                let mut prospective = aggregated.clone();
                prospective.resource_metrics.extend(transformed_message.resource_metrics.clone());
                let mut buf = Vec::new();
                if let Err(e) = prospective.encode(&mut buf) {
                    let err = format!("failed to encode prospective aggregated message: {}", e);
                    error!("{}", err);
                    return Err(err.into());
                }

                if buf.len() > config.batch_max_size_bytes && !aggregated.resource_metrics.is_empty() {
                    // Flush current aggregator first
                    let mut current_body = Vec::new();
                    if let Err(e) = aggregated.encode(&mut current_body) {
                        let err = format!("failed to encode aggregated message for flush: {}", e);
                        error!("{}", err);
                        return Err(err.into());
                    }
                    info!(
                        bytes = current_body.len(),
                        max_bytes = config.batch_max_size_bytes,
                        resource_metrics = aggregated.resource_metrics.len(),
                        "flushing aggregated metrics due to batch size threshold"
                    );
                    coralogix_send(&config, current_body)
                        .await
                        .map_err(|e| {
                            let err = format!("failed to send aggregated metric data to coralogix: {}", e);
                            error!("{}", err);
                            err
                        })?;

                    // Start a new batch with this transformed message (or send directly if still too big)
                    *aggregated = ExportMetricsServiceRequest { ..Default::default() };

                    // Check if the single transformed message alone exceeds the limit; if so, send alone
                    let mut single_body = Vec::new();
                    let mut single_only = ExportMetricsServiceRequest { ..Default::default() };
                    single_only.resource_metrics.extend(transformed_message.resource_metrics.clone());
                    if let Err(e) = single_only.encode(&mut single_body) {
                        let err = format!("failed to encode single transformed message: {}", e);
                        error!("{}", err);
                        return Err(err.into());
                    }
                    if single_body.len() > config.batch_max_size_bytes {
                        info!(
                            bytes = single_body.len(),
                            max_bytes = config.batch_max_size_bytes,
                            "single transformed message exceeds batch size; sending as-is"
                        );
                        coralogix_send(&config, single_body)
                            .await
                            .map_err(|e| {
                                let err = format!("failed to send oversize single metric payload to coralogix: {}", e);
                                error!("{}", err);
                                err
                            })?;
                    } else {
                        let before_rm = aggregated.resource_metrics.len();
                        aggregated.resource_metrics.extend(transformed_message.resource_metrics);
                        let after_rm = aggregated.resource_metrics.len();
                        debug!(
                            added_resource_metrics = (after_rm.saturating_sub(before_rm)),
                            total_resource_metrics = after_rm,
                            "batched metrics aggregated (new batch)"
                        );
                    }
                } else {
                    // Safe to add to existing batch
                    let before_rm = aggregated.resource_metrics.len();
                    aggregated.resource_metrics.extend(transformed_message.resource_metrics);
                    let after_rm = aggregated.resource_metrics.len();
                    debug!(
                        added_resource_metrics = (after_rm.saturating_sub(before_rm)),
                        total_resource_metrics = after_rm,
                        "batched metrics aggregated"
                    );
                }
            }
        } else {
            info!(per_record_messages = messages.len(), "batching disabled; sending individually");
            for (midx, message) in messages.into_iter().enumerate() {
                let transformed_message = transform_message(
                    message,
                    &config.app_name,
                    &config.sub_name,
                )
                .map_err(|e| {
                    let err = format!("failed to transform message: {}", e);
                    error!("{}", err);
                    err
                })?;

                let mut modified_message_vec = Vec::new();
                transformed_message
                    .encode(&mut modified_message_vec)
                    .map_err(|e| {
                        let err = format!("failed to encode modified message: {}", e);
                        error!("{}", err);
                        err
                    })?;
                debug!(record_index = idx, message_index = midx, bytes = modified_message_vec.len(), "sending single metrics payload");
                coralogix_send(&config, modified_message_vec)
                    .await
                    .map_err(|e| {
                        let err = format!("failed to send metric data to coralogix: {}", e);
                        error!("{}", err);
                        err
                    })?;
            }
        }

        // Add the record to the results to be dropped
        results.push(KinesisFirehoseResponseRecord {
            metadata: KinesisFirehoseResponseRecordMetadata {
                partition_keys: std::collections::HashMap::new(),
            }, 
            record_id: record.record_id,
            result: Some("Dropped".to_string()),
            data: record.data,
        });
    };
    // If batching enabled, send once for the full aggregated payload
    if config.batching_enabled {
        if let Some(aggregated) = aggregated_opt {
            if !aggregated.resource_metrics.is_empty() {
                // Compute some insight into the aggregated shape
                let total_resource_metrics = aggregated.resource_metrics.len();
                let mut total_scope_metrics = 0usize;
                let mut total_metrics = 0usize;
                for rm in &aggregated.resource_metrics {
                    total_scope_metrics += rm.scope_metrics.len();
                    for sm in &rm.scope_metrics {
                        total_metrics += sm.metrics.len();
                    }
                }

                let mut body = Vec::new();
                aggregated
                    .encode(&mut body)
                    .map_err(|e| {
                        let err = format!("failed to encode aggregated message: {}", e);
                        error!("{}", err);
                        err
                    })?;

                info!(
                    total_records = event.records.len(),
                    total_messages_seen,
                    total_resource_metrics,
                    total_scope_metrics,
                    total_metrics,
                    bytes = body.len(),
                    "sending aggregated metrics payload"
                );

                coralogix_send(&config, body)
                    .await
                    .map_err(|e| {
                        let err = format!("failed to send aggregated metric data to coralogix: {}", e);
                        error!("{}", err);
                        err
                    })?;
            } else {
                info!("batching enabled but no aggregated resource_metrics to send");
            }
        } else {
            info!("batching enabled but aggregator not initialized");
        }
    }

    // if all is well, return an empty response to firehose
    Ok(KinesisFirehoseResponse {
        records: results,
    })
}
