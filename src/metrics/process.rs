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
use tracing::{debug, error};


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
    let uri = &format!("{}/v1/metrics", config.endpoint);

    debug!("sending to metric data to uri: {:?}", uri);
    
    reqwest::Client::new()
        .post(uri)
        .header("Content-Type", "application/x-protobuf")
        .header(
            "Authorization",
            &format!("Bearer {}", config.api_key.token()),
        )
        .body(message)
        .send()
        .await?;

    Ok(())    
}

// transform_firehose_event - processes the KinesisFirehoseEvent and sends the transformed data to Coralogix
pub async fn transform_firehose_event(
    config: &Config,
    event: KinesisFirehoseEvent,
) -> Result<KinesisFirehoseResponse, Error> {
    // Iterate over all records in the KinesisFirehoseEvent
    let mut results = Vec::new();
    for record in event.records.clone() {
        let otel_payload = record.data.clone();
        let messages = split_length_delimited(&otel_payload.0)
            .map_err(|e| {
                let err = format!("failed to split length-delimited data: {}", e);
                error!("{}", err);
                err
            })?;
    
        for message in messages {            
            let transformed_message = transform_message(message, &config.app_name, &config.sub_name)
                .map_err(|e| {
                    let err = format!("failed to transform message: {}", e);
                    error!("{}", err);
                    err
                })?;
    
            let mut modified_message_vec = Vec::new();
            transformed_message.encode(&mut modified_message_vec)
                .map_err(|e| {
                    let err = format!("failed to encode modified message: {}", e);
                    error!("{}", err);
                    err
                })?;
      
            // TODO: look into whether these messages can be batched instead of sending one at a time via rebatch function
            coralogix_send(&config, modified_message_vec).await
                .map_err(|e| {
                    let err = format!("failed to send metric data to coralogix: {}", e);
                    error!("{}", err);
                    err
                })?;  
        };

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
    

    // if all is well, return an empty response to firehose
    Ok(KinesisFirehoseResponse {
        records: results,
    })
}
