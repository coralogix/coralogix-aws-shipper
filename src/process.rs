use aws_lambda_events::cloudwatch_logs::AwsLogs;
use aws_lambda_events::ecr_scan::EcrScanEvent;
use aws_lambda_events::encodings::Base64Data;
use aws_lambda_events::kafka::KafkaRecord;
use aws_sdk_s3::Client;
use aws_sdk_ecr::Client as EcrClient;
use cx_sdk_rest_logs::DynLogExporter;
use fancy_regex::Regex;
use flate2::read::MultiGzDecoder;
use itertools::Itertools;
use lambda_runtime::Error;
use std::ffi::OsStr;
use std::io::Read;
use std::ops::Range;
use std::path::Path;
use std::string::String;
use std::time::Instant;
use tracing::{debug, info};
use base64::prelude::*;


use crate::config::{Config, IntegrationType};
use crate::coralogix;
use crate::ecr;



pub async fn s3(
    s3_client: &Client,
    coralogix_exporter: DynLogExporter,
    config: &Config,
    bucket: String,
    key: String,
) -> Result<(), Error> {
    
    
    let mut metadata_instance = Metadata {
        stream_name: String::new(),
        bucket_name: String::new(),
        key_name: String::new(),
    };
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| bucket.to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| bucket.to_string());

    let key_str = key.clone();
    let key_path = Path::new(&key_str);

    let batches = match config.integration_type {
        // VPC Flow Logs Needs Prefix and Sufix to be exact AWSLogs/ and .log.gz
        IntegrationType::VpcFlow => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            metadata_instance.key_name = key.clone();
            metadata_instance.bucket_name = bucket;
            process_vpcflows(raw_data, config.sampling, &config.blocking_pattern, key).await?
        }
        IntegrationType::S3Csv => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            metadata_instance.key_name = key.clone();
            metadata_instance.bucket_name = bucket;
            process_csv(
                raw_data,
                config.sampling,
                key_path,
                &config.csv_delimiter,
                &config.blocking_pattern,
                key,
            )
            .await?
        }
        IntegrationType::CloudFront => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            metadata_instance.key_name = key.clone();
            metadata_instance.bucket_name = bucket;
            let cloudfront_delimiter: &str = "\t";
            process_csv(
                raw_data,
                config.sampling,
                key_path,
                cloudfront_delimiter,
                &config.blocking_pattern,
                key,
            )
            .await?
        }
        IntegrationType::S3 => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            metadata_instance.key_name = key.clone();
            metadata_instance.bucket_name = bucket;
            process_s3(
                raw_data,
                config.sampling,
                key_path,
                &config.newline_pattern,
                &config.blocking_pattern,
                key,
            )
            .await?
        }
        IntegrationType::CloudTrail => {
            let raw_data = get_bytes_from_s3(s3_client, bucket.clone(), key.clone()).await?;
            metadata_instance.key_name = key.clone();
            metadata_instance.bucket_name = bucket;
            process_cloudtrail(raw_data, config.sampling, &config.blocking_pattern, key).await?
        }
        _ => {
            tracing::warn!(
                "Received event doesn't match the configured integration type: {:?}",
                config.integration_type
            );
            Vec::new()
        }
    };

    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    )
    .await?;

    Ok(())
}

trait MetadataTrait {
    fn get_stream_name(&self) -> String;
    fn get_bucket_name(&self) -> String;
    fn get_key_name(&self) -> String;
}

pub struct Metadata {
    pub stream_name: String,
    pub bucket_name: String,
    pub key_name: String,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            stream_name: String::new(),
            bucket_name: String::new(),
            key_name: String::new(),
        }
    }
}

fn is_gzipped(data: &[u8]) -> bool {
    // Check the first two bytes for gzip magic numbers
    data.len() > 1 && data[0] == 0x1f && data[1] == 0x8b
}
pub async fn kinesis_logs(
    kinesis_message: Base64Data,
    coralogix_exporter: DynLogExporter,
    config: &Config,
) -> Result<(), Error> {
    let metadata_instance = Metadata {
        stream_name: String::new(),
        bucket_name: String::new(),
        key_name: String::new(),
    };
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    let v = &kinesis_message.0;
    let string_data: Vec<u8> = if is_gzipped(v) {
        // It looks like gzip, attempt to ungzip
        match ungzip(v.clone(), String::new()) {
            Ok(un_v) => un_v,
            Err(_) => {
                tracing::error!("Data does not appear to be valid gzip format. Treating as UTF-8");
                v.clone()
            }
        }
    } else {
        // Not gzip, treat as UTF-8
        v.clone()
    };

    let batches = match String::from_utf8(string_data) {
        Ok(s) => {
            tracing::debug!("Kinesis Message: {:?}", s);
            vec![s]
        }
        Err(error) => {
            tracing::error!(?error ,"Failed to decode data");
            Vec::new()
        }
    };
    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    )
    .await?;
    Ok(())
}

pub async fn sqs_logs(
    sqs_message: String,
    coralogix_exporter: DynLogExporter,
    config: &Config,
) -> Result<(), Error> {
    let metadata_instance = Metadata {
        stream_name: String::new(),
        bucket_name: String::new(),
        key_name: String::new(),
    };
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    let mut batches = Vec::new();
    tracing::debug!("SNS Message: {:?}", sqs_message);
    batches.push(sqs_message.clone());
    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    )
    .await?;
    Ok(())
}
pub async fn sns_logs(
    sns_message: String,
    coralogix_exporter: DynLogExporter,
    config: &Config,
) -> Result<(), Error> {
    let metadata_instance = Metadata {
        stream_name: String::new(),
        bucket_name: String::new(),
        key_name: String::new(),
    };
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    let mut batches = Vec::new();
    tracing::debug!("SNS Message: {:?}", sns_message);
    batches.push(sns_message.clone());
    coralogix::process_batches(
        batches,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    )
    .await?;
    Ok(())
}
pub async fn cloudwatch_logs(
    cloudwatch_event_log: AwsLogs,
    coralogix_exporter: DynLogExporter,
    config: &Config,
) -> Result<(), Error> {
    let mut metadata_instance = Metadata {
        stream_name: String::new(),
        bucket_name: String::new(),
        key_name: String::new(),
    };
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = if let Some(sub_name) = &config.sub_name {
        if sub_name.is_empty() {
            cloudwatch_event_log.data.log_group.to_string()
        } else {
            sub_name.clone()
        }
    } else {
        cloudwatch_event_log.data.log_group.to_string()
    };

    let logs = match config.integration_type {
        IntegrationType::CloudWatch => {
            metadata_instance.stream_name = cloudwatch_event_log.data.log_stream.clone();
            process_cloudwatch_logs(cloudwatch_event_log, config.sampling).await?
        }
        _ => {
            tracing::warn!(
                "Received event doesn't match the configured integration type: {:?}",
                config.integration_type
            );
            Vec::new()
        }
    };

    coralogix::process_batches(
        logs,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    )
    .await?;

    Ok(())
}

pub async fn ecr_scan_logs(
    ecr_client: &EcrClient,
    ecr_scan_event: EcrScanEvent,
    coralogix_exporter: DynLogExporter,
    config: &Config,
) -> Result<(), Error> {
    let metadata_instance = Metadata {
        stream_name: String::new(),
        bucket_name: String::new(),
        key_name: String::new(),
    };
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());
    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());
    //let mut batches = Vec::new();
    
    tracing::debug!("ECR Scan Event: {:?}", ecr_scan_event);
    let payload = ecr::process_ecr_scan_event(ecr_scan_event, config, &ecr_client).await?;
    coralogix::process_batches(
        payload,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    ).await?;
    Ok(())
}


pub async fn get_bytes_from_s3(
    s3_client: &Client,
    bucket: String,
    key: String,
) -> Result<Vec<u8>, Error> {
    let start_time = Instant::now();
    let request = s3_client
        .get_object()
        .bucket(bucket.clone())
        .key(key.clone())
        .response_content_type("application/json");
    let response = request.send().await?;
    tracing::info!(
        "Received response from S3 in {}ms",
        start_time.elapsed().as_millis()
    );

    // Downloading the content this way is faster and allocates less memory than using `body.collect().await?.to_vec()`
    let mut data = Vec::with_capacity(response.content_length.unwrap_or(1024 * 1024) as usize);
    let mut body = response.body;
    while let Some(result) = body.next().await {
        let bytes = result?;
        data.extend_from_slice(&bytes[..])
    }

    tracing::info!(
        "Downloaded file from S3 in {}ms. Actual size: {} bytes. Name of the file: {}",
        start_time.elapsed().as_millis(),
        data.len(),
        key
    );

    Ok(data)
}

async fn process_cloudwatch_logs(cw_event: AwsLogs, sampling: usize) -> Result<Vec<String>, Error> {
    let log_entries: Vec<String> = cw_event
        .data
        .log_events
        .into_iter()
        .map(|entry| entry.message)
        .collect();
    info!("Received {} CloudWatch logs", log_entries.len());
    debug!("Cloudwatch Logs: {:?}", log_entries);
    Ok(sample(sampling, log_entries))
}
async fn process_vpcflows(
    raw_data: Vec<u8>,
    sampling: usize,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    info!("VPC Flow Integration Type");
    let v = ungzip(raw_data, key)?;
    let s = String::from_utf8(v)?;
    let array_s = split(Regex::new(r"\n")?, s.as_str())?;
    let flow_header = split(Regex::new(r"\s+")?, array_s[0])?;
    let csv_delimiter = " ";
    let records: Vec<&str> = array_s
        .iter()
        .skip(1)
        .filter(|&line| !line.trim().is_empty())
        .copied()
        .collect_vec();
    let parsed_records = parse_records(&flow_header, &records, csv_delimiter)?;
    let re_block: Regex = Regex::new(blocking_pattern)?;
    tracing::debug!("Parsed Records: {:?}", &parsed_records);
    Ok(sample(
        sampling,
        block(re_block, parsed_records, blocking_pattern)?,
    ))
}

async fn process_csv(
    raw_data: Vec<u8>,
    sampling: usize,
    key_path: &Path,
    mut csv_delimiter: &str,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    info!("CSV Integration Type");
    if csv_delimiter == "\\t" {
        debug!("Replacing \\t with \t");
        csv_delimiter = "\t";}
    let s = if key_path.extension() == Some(OsStr::new("gz")) {
        let v = ungzip(raw_data, key)?;
        let s = String::from_utf8(v)?;
        debug!("ZIP S3 object: {}", s);
        s
    } else {
        let s = String::from_utf8(raw_data)?;
        debug!("NON-ZIP S3 object: {}", s);
        s
    };
    let mut flow_header: Vec<&str>;
    let array_s = split(Regex::new(r"\n")?, s.as_str())?;
    let records: Vec<&str> = if array_s[0].starts_with("#Version") {
        debug!("Array 0: {:?}", &array_s[0]);
        flow_header = array_s[1].split(' ').collect_vec();
        flow_header.remove(0);
        tracing::debug!("Flow Header: {:?}", &flow_header);
        array_s
            .iter()
            .skip(2)
            .filter(|&line| !line.trim().is_empty())
            .copied()
            .collect_vec()
    } else {
        flow_header = array_s[0].split(csv_delimiter).collect_vec();
        tracing::debug!("Flow Header: {:?}", &flow_header);
        array_s
            .iter()
            .skip(1)
            .filter(|&line| !line.trim().is_empty())
            .copied()
            .collect_vec()
    };
    
    let re_block: Regex = Regex::new(blocking_pattern)?;
    let parsed_records = parse_records(&flow_header, &records, csv_delimiter)?;
    debug!("Parsed Records: {:?}", &parsed_records);
    Ok(sample(
        sampling,
        block(re_block, parsed_records, blocking_pattern)?,
    ))
}

async fn process_s3(
    raw_data: Vec<u8>,
    sampling: usize,
    key_path: &Path,
    newline_pattern: &str,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    let s = if key_path.extension() == Some(OsStr::new("gz")) {
        let v = ungzip(raw_data, key)?;
        let s = String::from_utf8(v)?;
        debug!("ZIP S3 object: {}", s);
        s
    } else {
        let s = String::from_utf8(raw_data)?;
        debug!("NON-ZIP S3 object: {}", s);
        s
    };
    let pattern = if newline_pattern.is_empty() {
        debug!("Newline pattern not set, using default");
        r"\n"
    } else {
        newline_pattern
    };
    let re = Regex::new(pattern)?;
    let re_block: Regex = Regex::new(blocking_pattern)?;
    debug!("OUT S3 object: {}", s);
    let logs: Vec<String> = sample(
        sampling,
        block(
            re_block,
            split(re, &s)?
                .into_iter()
                .map(|line| line.to_owned())
                .collect_vec(),
            blocking_pattern,
        )?, // TODO Can we avoid copying?
    );

    Ok(logs)
}

async fn process_cloudtrail(
    raw_data: Vec<u8>,
    sampling: usize,
    blocking_pattern: &str,
    key: String,
) -> Result<Vec<String>, Error> {
    tracing::info!("Cloudtrail Integration Type");
    let v = ungzip(raw_data, key)?;
    let s = String::from_utf8(v)?;
    let mut logs_vec: Vec<String> = Vec::new();
    let array_s = serde_json::from_str::<serde_json::Value>(&s)?;
    tracing::debug!(
        "Array: {}",
        serde_json::to_string(&array_s).unwrap_or("".to_string())
    );
    let re_block: Regex = Regex::new(blocking_pattern)?;
    if let Some(records) = array_s.get("Records") {
        if let Some(records_array) = records.as_array() {
            for log in records_array.iter().map(|log| log.to_string()) {
                tracing::debug!("Logs: {}", log);
                logs_vec.push(log.to_string());
            }
            tracing::debug!("OUT S3 object: {}", s);
            let logs: Vec<String> = sample(sampling, block(re_block, logs_vec, blocking_pattern)?);

            Ok(logs)
        } else {
            Ok(Vec::new())
        }
    } else {
        tracing::warn!("Non Cloudtrail Log Ingested - Skipping - {}", s);
        Ok(Vec::new())
    }
}

pub async fn kafka_logs(
    records:  Vec<KafkaRecord>,
    coralogix_exporter: DynLogExporter,
    config: &Config,
) -> Result<(), Error> {
    let defined_app_name = config
        .app_name
        .clone()
        .unwrap_or_else(|| "NO APPLICATION NAME".to_string());

    let defined_sub_name = config
        .sub_name
        .clone()
        .unwrap_or_else(|| "NO SUBSYSTEM NAME".to_string());

    let mut batch = Vec::new();
    for record in records {
        if let Some(value) = record.value {
            // check if value is base64 encoded
            if let Ok(message) = BASE64_STANDARD.decode(&value) {
                batch.push(String::from_utf8(message)?);
            } else {
                batch.push(value);
            }
        }
    }

    let metadata_instance = Metadata::default();

    coralogix::process_batches(
        batch,
        &defined_app_name,
        &defined_sub_name,
        config,
        &metadata_instance,
        coralogix_exporter,
    )
    .await?;

    Ok(())
}

fn ungzip(compressed_data: Vec<u8>, key: String) -> Result<Vec<u8>, Error> {
    if compressed_data.is_empty() {
        tracing::warn!("Input data is empty, cannot ungzip a zero-byte file.");
        return Ok(Vec::new());
    }
    let mut d = MultiGzDecoder::new(&compressed_data[..]);
    let mut v = Vec::new();
    match d.read_to_end(&mut v) {
        Ok(_) => Ok(v),
        Err(e) => {
            tracing::error!(
                "Failed to ungzip data from  Key_Path: {}. Error: {}",
                key,
                e
            );
            Err(Box::new(e))
        }
    }
}

fn parse_records(
    flow_header: &[&str],
    records: &[&str],
    csv_delimiter: &str,
) -> Result<Vec<String>, String> {
    records
        .iter()
        .map(|record| {
            let values: Vec<&str> = record.split(csv_delimiter).collect();
            let mut parsed_log = serde_json::Map::new();
            debug!("DELIMITER: {:?}", csv_delimiter);
            for (index, field) in flow_header.iter().enumerate() {
                // Use a default empty string if the value is missing
                let value = values.get(index).unwrap_or(&"");

                let parsed_value = if let Ok(num) = value.parse::<i32>() {
                    serde_json::Value::Number(serde_json::Number::from(num))
                } else {
                    serde_json::Value::String(value.to_string())
                };
                debug!("Parsed Value: {:?}", parsed_value);
                parsed_log.insert(field.to_string(), parsed_value);
            }

            serde_json::to_string(&parsed_log).map_err(|e| e.to_string())
        })
        .collect()
    
}

fn split(re: Regex, string: &str) -> Result<Vec<&str>, Error> {
    let mut logs: Vec<&str> = Vec::new();
    let mut next_log_start: usize = 0;

    for m in re.find_iter(string) {
        let Range { start, end } = m?.range();
        if start != 0 {
            // this will skip creating an empty log at the beginning
            logs.push(&string[next_log_start..start]);
        }
        next_log_start = end;
    }
    logs.push(&string[next_log_start..]);

    Ok(logs)
}

fn block(re_block: Regex, v: Vec<String>, b: &str) -> Result<Vec<String>, Error> {
    if b.is_empty() {
        return Ok(v);
    }
    let mut logs: Vec<String> = Vec::new();
    for l in v {
        if re_block.is_match(&l)? {
            tracing::warn!("Blocking pattern matched");
            tracing::debug!("Log Matched: {}", l)
        } else {
            tracing::debug!("Log Not Matched: {}", l);
            logs.push(l);
        }
    }
    Ok(logs)
}

fn sample(sampling: usize, v: Vec<String>) -> Vec<String> {
    v.into_iter().step_by(sampling).collect_vec()
}

#[cfg(test)]
mod test {
    use fancy_regex::Regex;
    use itertools::Itertools;

    use crate::process::{block, sample, split};

    #[test]
    fn test() {
        let log_file_contents = r#"09-24 16:09:07.042: ERROR1 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)
09-24 16:09:07.042: ERROR2 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)
09-24 16:09:07.042: ERROR3 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)"#;

        let regex = Regex::new(r#"\n(?=\d{2}\-\d{2}\s\d{2}\:\d{2}\:\d{2}\.\d{3})"#).unwrap();
        let regex_block: Regex = Regex::new(r#"ERROR3"#).unwrap();
        let blocking_pattern = "ERROR3";
        let sampling = 1;
        let logs = sample(
            sampling,
            block(
                regex_block,
                split(regex, log_file_contents)
                    .unwrap()
                    .into_iter()
                    .map(|l| l.to_owned())
                    .collect_vec(),
                blocking_pattern,
            )
            .unwrap(),
        );
        tracing::info!("{:?}", logs);
        assert_eq!(logs.len(), 2);
        assert_eq!(
            logs[0],
            r#"09-24 16:09:07.042: ERROR1 System.out(4844): java.lang.NullPointerException
at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)"#
        );
        //    assert_eq!(logs[1], r#"09-24 16:09:07.042: ERROR2 System.out(4844): java.lang.NullPointerException
        //at com.temp.ttscancel.MainActivity.onCreate(MainActivity.java:43)
        //at android.app.ActivityThread$H.handleMessage(ActivityThread.java:1210)"#);
    }
}
