use crate::process::Metadata;
use crate::*;
use serde::{Serialize, Deserialize};
use aws_lambda_events::cloudwatch_logs::AwsLogs;
use aws_lambda_events::ecr_scan::{EcrScanEvent, self};
use aws_lambda_events::encodings::Base64Data;
use aws_sdk_ecr::types::ImageScanFinding;
use aws_sdk_ecr::types::ImageIdentifier;
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
use tracing::{debug, info, warn};

use crate::config::{Config, IntegrationType};
use crate::coralogix;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Root {
    metadata: Ecrmetadata,
    finding: Findings,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Ecrmetadata {
    repository: String,
    image_id: ImageId,
    image_tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ImageId {
    image_digest: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Findings {
    package: Package,
    details: Details,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Package {
    package: String,
    name: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Details {
    name: String,
    uri: String,
    severity: String,
    attributes: Vec<Attribute>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Attribute {
    key: String,
    value: String,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
enum FindingSeverity {
    #[allow(missing_docs)] 
    Critical,
    #[allow(missing_docs)] 
    High,
    #[allow(missing_docs)] 
    Informational,
    #[allow(missing_docs)] 
    Low,
    #[allow(missing_docs)] 
    Medium,
    #[allow(missing_docs)] 
    Undefined,
}

pub async fn process_ecr_scan_event(
    event: EcrScanEvent,
    config: &Config,
    ecr_client: &EcrClient,
) -> Result<Vec<String>, Error> {
    let current_ecr_metadata = Ecrmetadata {
        repository: event.detail.repository_name.unwrap_or("".to_string()),
        image_id: ImageId {
            image_digest: event.detail.image_digest.unwrap_or("".to_string()),
        },
        image_tags: event.detail.image_tags,
    };
    let image_identifier = ImageIdentifier::builder()
        .image_digest(current_ecr_metadata.image_id.image_digest.clone())
        .build();

    let request = ecr_client
        .describe_image_scan_findings()
        .repository_name(current_ecr_metadata.repository.clone())
        .image_id(image_identifier)
        .send();
    let response = request.await?;

    let mut logs: Vec<String> = Vec::new(); // Initialize logs outside of the loop

    if let Some(findings) = response.image_scan_findings.unwrap().findings {
        for finding in findings {
            info!("Finding: {:?}", finding);
            if let Some(attributes) = finding.attributes {
                let mut current_attributes: Vec<Attribute> = Vec::new();
                let mut current_name: String = "NO_PACKAGE".to_string();
                let mut current_version: String = "NO_VERSION".to_string();
                for attribute in attributes {
                    info!("Attribute: {:?}", attribute);
                    let attribute_value = attribute.value; 
                    if current_name == "NO_PACKAGE" && attribute.key == "package_name" {
                        current_name = attribute_value.clone().unwrap_or("NO_PACKAGE".to_string());
                    }
                    if current_version == "NO_VERSION" {
                        if attribute.key == "package_version" {
                            current_version = attribute_value.clone().unwrap_or("NO_VERSION".to_string());
                        }
                    }
                    
                    current_attributes.push(Attribute {
                        key: attribute.key,
                        value: attribute_value.unwrap_or("".to_string()),
                    });
                    let package = Package {
                        package: format!("{}:{}", current_name.clone(), current_version),
                        name: current_name.clone(),
                        version: current_version.clone(),
                    };
                let current_details = Details {
                    name: finding.name.clone().unwrap_or("".to_string()),
                    uri: finding.uri.clone().unwrap_or("".to_string()),
                    severity: finding.severity.clone().map_or("".to_string(), |v| v.as_str().to_string()),
                    attributes: current_attributes.clone(),
                };
                let findings = Findings {
                    package: package,
                    details: current_details,
                };
                let root = Root {
                    metadata: current_ecr_metadata.clone(),
                    finding: findings,
                };
                info!("Root: {:?}", root);
                logs.push(serde_json::to_string(&root).unwrap());
                }  
            }   
        }
    }

    Ok(logs) // Return the logs as the result of the function
}