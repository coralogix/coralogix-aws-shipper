
use aws_lambda_events::event::firehose::KinesisFirehoseEvent;
use aws_lambda_events::firehose::KinesisFirehoseResponse;
use aws_lambda_events::firehose::KinesisFirehoseResponseRecord;
use aws_lambda_events::firehose::KinesisFirehoseResponseRecordMetadata;
use crate::metrics::config::Config;

pub async fn kinesis_firehose(config: &Config,  event: KinesisFirehoseEvent) {
    let mut response_records = Vec::new();

    // Iterate over all records in the KinesisFirehoseEvent
    for record in &event.records {
        let otel_payload = record.data.clone();
        let messages = match split_length_delimited(&otel_payload.0) {
            Ok(msgs) => msgs,
            Err(e) => {
                error!("Failed to split length-delimited data: {}", e);
                response_records.push(KinesisFirehoseResponseRecord {
                    record_id: Some(record.record_id.clone().unwrap_or_default()),
                    result: Some("ProcessingFailed".to_string()),
                    data: otel_payload.clone(),
                    metadata: KinesisFirehoseResponseRecordMetadata {
                        partition_keys: Default::default(),
                    },
                });
                continue;
            }
        };

        let mut processing_failed = false;

        for message in messages {
            let mut decoded_message = ExportMetricsServiceRequest::decode(&*message)?;
            //debug!("Decoded Metrics: {:?}", decoded_message);

            for resource in decoded_message.resource_metrics.iter_mut() {
                //debug!("Listing Resources: {:?}", resource);
                for scope_metrics in resource.scope_metrics.iter_mut() {
                    //debug!("Scope Metrics Iter: {:?}", scope_metrics.metrics.iter());
                    for metric in scope_metrics.metrics.iter_mut() {  
                        //metric.unit = curly_braces_re.replace_all(&metric.unit, "").to_string().to_lowercase();
                        metric.unit = "".to_string();
                        debug!("Metric Metadata: {:?}", metric.data);
                        if let Some(data) = &mut metric.data {
                            match data {
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(gauge) => {
                                    debug!("Gauge: {:?}", gauge);
                                }
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(sum) => {
                                    debug!("Sum: {:?}", sum);
                                }
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(histogram) => {
                                    debug!("Histogram: {:?}", histogram);
                                }
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::ExponentialHistogram(exponential_histogram) => {
                                    debug!("ExponentialHistogram: {:?}", exponential_histogram);
                                }
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

                                            // Add cx.application.name and cx.subsystem.name attributes
                                            
                                        });
                                        new_attributes.push(KeyValue {
                                            key: "cx.application.name".to_string(),
                                            value: Some(AnyValue {
                                                value: Some(any_value::Value::StringValue(app_name.clone())),
                                            }),
                                        });
                                        new_attributes.push(KeyValue {
                                            key: "cx.subsystem.name".to_string(),
                                            value: Some(AnyValue {
                                                value: Some(any_value::Value::StringValue(sub_name.clone())),
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

            let mut modified_message_vec = Vec::new();
            if let Err(e) = decoded_message.encode(&mut modified_message_vec) {
                error!("Failed to encode modified message: {}", e);
                processing_failed = true;
                break;
            }

            let mut attempts = 0;
            let res = loop {
                let res = reqwest::Client::new()
                    .post(&format!("https://{}/v1/metrics", endpoint))
                    .header("Content-Type", "application/x-protobuf")
                    .header("Authorization", &format!("Bearer {}", api_key))
                    .header("cx.application.name", &app_name)
                    .header("cx.subsystem.name", &sub_name)
                    .body(modified_message_vec.clone())
                    .send()
                    .await;

                match res {
                    Ok(response) if response.status().is_success() => {
                        break Ok(response);
                    }
                    Ok(response) => {
                        error!(
                            "Request failed with status: {:?}. Retry count {:?}",
                            response.status(),
                            attempts
                        );

                        if attempts >= retry_limit {
                            error!("Dropping after {:?} retries", attempts);
                            break Err(Error::from("Request failed after retries"));
                        }
                    }
                    Err(e) => {
                        error!("Request error: {:?}", e);
                        if attempts >= retry_limit {
                            error!("Dropping after {:?} retries", attempts);
                            break Err(Error::from("Request failed after retries"));
                        }
                    }
                }

                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_secs(retry_delay)).await;
            };

            match res {
                Ok(response) => {
                    info!("Response: {:?}", response);
                    match response.text().await {
                        Ok(text) => {
                            debug!("Response text: {:?}", text);
                        }
                        Err(e) => {
                            error!("Failed to get response text: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Request failed after retries: {:?}", e);
                    processing_failed = true;
                    break;
                }
            }
        }

        let result = if processing_failed {
            "ProcessingFailed".to_string()
        } else {
            "Ok".to_string()
        };

        response_records.push(KinesisFirehoseResponseRecord {
            record_id: Some(record.record_id.clone().unwrap_or_default()),
            result: Some(result),
            data: otel_payload.clone(),
            metadata: KinesisFirehoseResponseRecordMetadata {
                partition_keys: Default::default(),
            },
        });
    }

    Ok(KinesisFirehoseResponse {
        records: response_records,
    })
}