use serde::{Serialize, Deserialize};
use aws_lambda_events::ecr_scan::EcrScanEvent;
use aws_sdk_ecr::Client as EcrClient;
use lambda_runtime::Error;
use std::string::String;
use tracing::info;
use aws_sdk_ecr::types::ImageIdentifier;
use tracing::debug;
use crate::config::Config;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct CurrentCountRoot {
    metadata: EcrMetadata,
    ecr_scan_summary: EcrScanSummary,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct EcrScanSummary {
    finding_severity_counts: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Root {
    metadata: EcrMetadata,
    findings: Findings,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct EcrMetadata {
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
    _config: &Config,
    ecr_client: &EcrClient,
) -> Result<Vec<String>, Error> {
    let current_severity_count = event.detail.finding_severity_counts;
    info!("current_severity_count: {:?}", current_severity_count);
    let current_ecr_metadata = EcrMetadata {
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
    let response  = request.await?;
    debug!("ECR response: {:?}", response);
    let image_scan_findings = match response.image_scan_findings {
        Some(findings) => findings,
        None => return Err(Error::from("No image scan findings available")),
    };

    let mut logs: Vec<String> = Vec::new(); // Initialize logs outside of the loop

    if let Some(findings) = image_scan_findings.findings {
        for finding in findings {
            if let Some(attributes) = finding.attributes {
                let mut current_attributes: Vec<Attribute> = Vec::new();
                let mut current_name: String = "NO_PACKAGE".to_string();
                let mut current_version: String = "NO_VERSION".to_string();
                for attribute in attributes {
                    let attribute_value = attribute.value; 
                    if current_name == "NO_PACKAGE" && attribute.key == "package_name" {
                        current_name = attribute_value.clone().unwrap_or_else(|| "NO_PACKAGE".to_string());
                    }
                    if current_version == "NO_VERSION" && attribute.key == "package_version"{
                        current_version = attribute_value.clone().unwrap_or_else(|| "NO_VERSION".to_string());
                    }
                    
                    current_attributes.push(Attribute {
                        key: attribute.key,
                        value: attribute_value.unwrap_or("".to_string()),
                    });
                }
            let package = Package {
                package: format!("{}:{}", current_name, current_version),
                name: current_name,
                version: current_version,
            };
                   
            let current_details = Details {
                name: finding.name.unwrap_or_default(),
                uri: finding.uri.unwrap_or_default(),
                severity: finding.severity.map_or_else(|| "".to_string(), |v| v.as_str().to_string()),
                attributes: current_attributes,
            };
            let findings = Findings {
                package: package,
                details: current_details,
            };
            let root = Root {
                metadata: current_ecr_metadata.clone(),
                findings: findings,
            };
            logs.push(serde_json::to_string(&root).unwrap_or_else(|_| "Failed to serialize root".to_string()));
            }  
        }   
        if let Some(finding_severity_counts) = image_scan_findings.finding_severity_counts {
            let finding_severity_counts: Vec<String> = finding_severity_counts.iter()
                .map(|(severity, count)| format!("{:?}: {}", severity, count))
                .collect();
            let ecr_scan_summary: EcrScanSummary = EcrScanSummary {
                finding_severity_counts: finding_severity_counts,
            };
            let current_count_root = CurrentCountRoot {
                metadata: current_ecr_metadata,
                ecr_scan_summary: ecr_scan_summary,
            };
        logs.push(serde_json::to_string(&current_count_root).unwrap_or_else(|_| "Failed to serialize current_count_root".to_string()));
        }
    }
    
    Ok(logs)
}