//! AWS resource tag enrichment for CloudWatch metric streams (YACE-compatible associator).

use aws_sdk_resourcegroupstagging::Client as RgtClient;
use opentelemetry_proto::tonic::common::v1::any_value;
use opentelemetry_proto::tonic::common::v1::AnyValue;
use opentelemetry_proto::tonic::common::v1::KeyValue;
use opentelemetry_proto::tonic::metrics::v1::SummaryDataPoint;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use tracing::{debug, error, warn};

/// Prometheus/common `LabelsToSignature` (FNV-1a + string hash), matching YACE associator.
fn labels_to_signature(labels: &HashMap<String, String>) -> u64 {
    if labels.is_empty() {
        return 0;
    }
    let mut names: Vec<&str> = labels.keys().map(String::as_str).collect();
    names.sort();

    const SEPARATOR: u8 = 0xfe;
    let mut sum: u64 = 14695981039346656037;
    for name in names {
        sum = hash_add(sum, hash_new_string(name));
        sum = hash_add_byte(sum, SEPARATOR);
        sum = hash_add(
            sum,
            hash_new_string(labels.get(name).map(String::as_str).unwrap_or("")),
        );
        sum = hash_add_byte(sum, SEPARATOR);
    }
    sum
}

#[inline]
fn hash_add(a: u64, b: u64) -> u64 {
    a ^ b
}

#[inline]
fn hash_add_byte(a: u64, b: u8) -> u64 {
    hash_add(a, b as u64)
}

fn hash_new_string(s: &str) -> u64 {
    let mut sum: u64 = 5381;
    for c in s.chars() {
        sum = sum.wrapping_mul(33).wrapping_add(c as u64);
    }
    sum
}

#[derive(Clone)]
struct DimensionsRegexpMapping {
    dimension_names: Vec<String>,
    dimensions_mapping: HashMap<u64, TaggedResource>,
}

#[derive(Clone)]
struct TaggedResource {
    arn: String,
    tags: Vec<(String, String)>,
}

pub struct Associator {
    mappings: Vec<DimensionsRegexpMapping>,
}

impl Associator {
    fn new(dimension_regexes: &[Regex], resources: &[TaggedResource]) -> Self {
        let mut mapped = vec![false; resources.len()];
        let mut mappings = Vec::new();

        for regex in dimension_regexes {
            let dimension_names: Vec<String> = regex
                .capture_names()
                .filter_map(|n| n.filter(|x| !x.is_empty()))
                .map(|n| n.replace('_', " "))
                .collect();

            let mut m = DimensionsRegexpMapping {
                dimension_names,
                dimensions_mapping: HashMap::new(),
            };

            if m.dimension_names.is_empty() {
                continue;
            }

            for (idx, r) in resources.iter().enumerate() {
                if mapped[idx] {
                    continue;
                }
                let Some(caps) = regex.captures(&r.arn) else {
                    continue;
                };
                let mut labels = HashMap::new();
                for name_opt in regex.capture_names() {
                    if let Some(name) = name_opt {
                        if name.is_empty() {
                            continue;
                        }
                        let disp = name.replace('_', " ");
                        if let Some(mat) = caps.name(name) {
                            labels.insert(disp, mat.as_str().to_string());
                        }
                    }
                }
                if labels.is_empty() {
                    continue;
                }
                let sig = labels_to_signature(&labels);
                m.dimensions_mapping.insert(sig, r.clone());
                mapped[idx] = true;
            }

            if !m.dimensions_mapping.is_empty() {
                mappings.push(m);
            }
        }

        mappings.sort_by(|a, b| b.dimension_names.len().cmp(&a.dimension_names.len()));

        Associator { mappings }
    }

    fn associate_metric_to_resource(
        &self,
        namespace: &str,
        metric_dims: &[(String, String)],
    ) -> Option<TaggedResource> {
        if metric_dims.is_empty() {
            return None;
        }

        let dim_names: Vec<&str> = metric_dims.iter().map(|(k, _)| k.as_str()).collect();

        for regexp_mapping in &self.mappings {
            if !contains_all(&dim_names, &regexp_mapping.dimension_names) {
                continue;
            }
            let mut labels = HashMap::new();
            for r_dim in &regexp_mapping.dimension_names {
                for (name, value) in metric_dims {
                    let mut v = value.clone();
                    if namespace == "AWS/AmazonMQ" && name == "Broker" {
                        v = Regex::new(r"-[0-9]+$")
                            .map(|re| re.replace_all(&v, "").to_string())
                            .unwrap_or(v);
                    }
                    if r_dim == name {
                        labels.insert(name.clone(), v);
                    }
                }
            }
            let sig = labels_to_signature(&labels);
            if let Some(res) = regexp_mapping.dimensions_mapping.get(&sig) {
                return Some(res.clone());
            }
        }

        None
    }
}

fn contains_all<'a>(a: &[&'a str], b: &[String]) -> bool {
    b.iter().all(|e| a.iter().any(|x| *x == e.as_str()))
}

#[derive(Debug, Deserialize)]
struct YaceFile {
    #[serde(default)]
    services: Vec<YaceServiceJson>,
}

#[derive(Debug, Deserialize)]
struct YaceServiceJson {
    namespace: String,
    #[serde(default)]
    resource_filters: Vec<String>,
    #[serde(default)]
    dimension_regexes: Vec<String>,
}

#[derive(Clone)]
pub struct YaceService {
    pub namespace: String,
    pub resource_filters: Vec<String>,
    pub dimension_regexes: Vec<Regex>,
}

fn load_yace_services() -> Vec<YaceService> {
    const JSON: &str = include_str!("yace_services.json");
    let file: YaceFile = match serde_json::from_str(JSON) {
        Ok(f) => f,
        Err(e) => {
            error!("failed to parse embedded yace_services.json: {}", e);
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    for s in file.services {
        let mut compiled = Vec::new();
        for pat in &s.dimension_regexes {
            match Regex::new(pat) {
                Ok(r) => compiled.push(r),
                Err(e) => warn!(
                    namespace = %s.namespace,
                    pattern = %pat,
                    error = %e,
                    "skipping invalid YACE dimension regex"
                ),
            }
        }
        out.push(YaceService {
            namespace: s.namespace,
            resource_filters: s.resource_filters,
            dimension_regexes: compiled,
        });
    }
    out
}

fn yace_registry() -> &'static [YaceService] {
    static REG: OnceLock<Vec<YaceService>> = OnceLock::new();
    REG.get_or_init(load_yace_services)
}

pub fn service_for_namespace(namespace: &str) -> Option<&'static YaceService> {
    yace_registry()
        .iter()
        .find(|s| s.namespace == namespace)
}

/// Per-invocation cache: one associator per CloudWatch namespace.
#[derive(Default)]
pub struct NamespaceResourceCache {
    associators: HashMap<&'static str, Associator>,
}

impl NamespaceResourceCache {
    pub async fn associator_for(
        &mut self,
        client: &RgtClient,
        svc: &'static YaceService,
        file_cache_enabled: bool,
        file_cache_path: &str,
        file_cache_ttl: std::time::Duration,
        continue_on_resource_failure: bool,
    ) -> Result<&Associator, String> {
        let key = svc.namespace.as_str();
        if !self.associators.contains_key(key) {
            if svc.resource_filters.is_empty() || svc.dimension_regexes.is_empty() {
                self.associators
                    .insert(key, Associator::new(&svc.dimension_regexes, &[]));
            } else {
                let resources = match get_or_fetch_resources(
                    client,
                    svc,
                    file_cache_enabled,
                    file_cache_path,
                    file_cache_ttl,
                )
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        if continue_on_resource_failure {
                            warn!(namespace = %svc.namespace, error = %e, "tagging API failure; skipping AWS tags for namespace");
                            Vec::new()
                        } else {
                            return Err(e);
                        }
                    }
                };
                let assoc = Associator::new(&svc.dimension_regexes, &resources);
                self.associators.insert(key, assoc);
            }
        }
        Ok(self.associators.get(key).expect("associator inserted above"))
    }
}

fn cw_metric_from_attributes(attrs: &[KeyValue]) -> (String, String, Vec<(String, String)>) {
    let mut name = String::new();
    let mut namespace = String::new();
    let mut dims = Vec::new();

    for kv in attrs {
        match kv.key.as_str() {
            "MetricName" => name = attribute_string_value(kv),
            "Namespace" => namespace = attribute_string_value(kv),
            "Dimensions" => continue,
            "cx.application.name" | "cx.subsystem.name" => continue,
            k if k.starts_with("cx.") => continue,
            _ => dims.push((kv.key.clone(), attribute_string_value(kv))),
        }
    }
    (namespace, name, dims)
}

fn attribute_string_value(kv: &KeyValue) -> String {
    match &kv.value {
        Some(AnyValue {
            value: Some(any_value::Value::StringValue(s)),
        }) => s.clone(),
        Some(AnyValue {
            value: Some(any_value::Value::IntValue(i)),
        }) => i.to_string(),
        Some(AnyValue {
            value: Some(any_value::Value::DoubleValue(f)),
        }) => f.to_string(),
        Some(AnyValue {
            value: Some(any_value::Value::BoolValue(b)),
        }) => b.to_string(),
        _ => String::new(),
    }
}

async fn fetch_tagged_resources(
    client: &RgtClient,
    resource_type_filters: &[String],
) -> Result<Vec<TaggedResource>, String> {
    if resource_type_filters.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let mut token: Option<String> = None;

    loop {
        let mut req = client
            .get_resources()
            .set_resource_type_filters(Some(resource_type_filters.to_vec()));
        if let Some(ref t) = token {
            req = req.pagination_token(t);
        }

        let resp = req.send().await.map_err(|e| e.to_string())?;
        for m in resp.resource_tag_mapping_list() {
            let arn = m.resource_arn().unwrap_or("").to_string();
            let tags: Vec<(String, String)> = m
                .tags()
                .iter()
                .map(|t| (t.key().to_string(), t.value().to_string()))
                .collect();
            out.push(TaggedResource { arn, tags });
        }
        token = resp.pagination_token().map(|s| s.to_string());
        if token.as_ref().map_or(true, |t| t.is_empty()) {
            break;
        }
    }

    Ok(out)
}

#[derive(Deserialize)]
struct CachedResource {
    arn: String,
    tags: Vec<TagPair>,
}

#[derive(Deserialize)]
struct TagPair {
    key: String,
    value: String,
}

fn read_file_cache(path: &str) -> Option<Vec<TaggedResource>> {
    let data = std::fs::read(path).ok()?;
    let parsed: Vec<CachedResource> = serde_json::from_slice(&data).ok()?;
    Some(
        parsed
            .into_iter()
            .map(|r| TaggedResource {
                arn: r.arn,
                tags: r.tags.into_iter().map(|t| (t.key, t.value)).collect(),
            })
            .collect(),
    )
}

fn write_file_cache(path: &str, resources: &[TaggedResource]) -> Result<(), String> {
    let tmp = format!("{}.tmp", path);
    let tagged: Vec<serde_json::Value> = resources
        .iter()
        .map(|r| {
            serde_json::json!({
                "arn": r.arn,
                "tags": r.tags.iter().map(|(k,v)| serde_json::json!({"key": k, "value": v})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let bytes = serde_json::to_vec(&tagged).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

async fn get_or_fetch_resources(
    client: &RgtClient,
    svc: &'static YaceService,
    file_cache_enabled: bool,
    file_cache_path: &str,
    cache_ttl: std::time::Duration,
) -> Result<Vec<TaggedResource>, String> {
    if svc.resource_filters.is_empty() {
        return Ok(Vec::new());
    }

    let safe_ns = svc.namespace.replace('/', "-");
    let file_path = format!("{}/cache-{}", file_cache_path.trim_end_matches('/'), safe_ns);

    if file_cache_enabled {
        if let Ok(meta) = std::fs::metadata(&file_path) {
            if let Ok(modified) = meta.modified() {
                if modified.elapsed().unwrap_or(std::time::Duration::MAX) < cache_ttl {
                    if let Some(r) = read_file_cache(&file_path) {
                        debug!(namespace = %svc.namespace, "using file-cached tagged resources");
                        return Ok(r);
                    }
                }
            }
        }
    }

    match fetch_tagged_resources(client, &svc.resource_filters).await {
        Ok(resources) => {
            if file_cache_enabled && !resources.is_empty() {
                if let Err(e) = write_file_cache(&file_path, &resources) {
                    warn!(namespace = %svc.namespace, error = %e, "failed to write resource file cache");
                }
            }
            Ok(resources)
        }
        Err(e) => {
            if file_cache_enabled {
                if let Some(stale) = read_file_cache(&file_path) {
                    warn!(
                        namespace = %svc.namespace,
                        error = %e,
                        "GetResources failed; using stale file cache for tagged resources"
                    );
                    return Ok(stale);
                }
            }
            Err(e)
        }
    }
}

pub async fn enrich_summary_datapoint(
    client: &RgtClient,
    dp: &mut SummaryDataPoint,
    cache: &mut NamespaceResourceCache,
    file_cache_enabled: bool,
    file_cache_path: &str,
    file_cache_ttl: std::time::Duration,
    continue_on_resource_failure: bool,
    custom_metadata: &HashMap<String, String>,
) -> Result<(), String> {
    let (namespace, _metric_name, dims) = cw_metric_from_attributes(&dp.attributes);
    if namespace.is_empty() {
        append_static_labels(dp, custom_metadata);
        return Ok(());
    }

    let Some(svc) = service_for_namespace(&namespace) else {
        debug!(namespace = %namespace, "unsupported namespace for tag enrichment");
        append_static_labels(dp, custom_metadata);
        return Ok(());
    };

    let associator = cache
        .associator_for(
            client,
            svc,
            file_cache_enabled,
            file_cache_path,
            file_cache_ttl,
            continue_on_resource_failure,
        )
        .await?;

    if let Some(r) = associator.associate_metric_to_resource(&namespace, &dims) {
        for (k, v) in &r.tags {
            dp.attributes.push(KeyValue {
                key: k.clone(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue(v.clone())),
                }),
            });
        }
    }

    append_static_labels(dp, custom_metadata);
    Ok(())
}

/// Append `CUSTOM_METADATA` labels only (when AWS tag enrichment is disabled).
pub fn apply_custom_metadata_labels(
    dp: &mut SummaryDataPoint,
    custom_metadata: &HashMap<String, String>,
) {
    append_static_labels(dp, custom_metadata);
}

fn append_static_labels(dp: &mut SummaryDataPoint, custom_metadata: &HashMap<String, String>) {
    for (k, v) in custom_metadata {
        dp.attributes.push(KeyValue {
            key: k.clone(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(v.clone())),
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::metrics::v1::SummaryDataPoint;

    fn string_attr(key: &str, value: &str) -> KeyValue {
        KeyValue {
            key: key.to_string(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue(value.to_string())),
            }),
        }
    }

    #[test]
    fn labels_to_signature_stable_under_key_reordering() {
        let mut a = HashMap::new();
        a.insert("dim_a".to_string(), "1".to_string());
        a.insert("dim_b".to_string(), "2".to_string());
        let mut b = HashMap::new();
        b.insert("dim_b".to_string(), "2".to_string());
        b.insert("dim_a".to_string(), "1".to_string());
        assert_eq!(labels_to_signature(&a), labels_to_signature(&b));
    }

    #[test]
    fn associator_matches_ec2_instance_by_dimension() {
        let re = Regex::new(r"instance/(?P<InstanceId>[^/]+)").unwrap();
        let resources = vec![TaggedResource {
            arn: "arn:aws:ec2:eu-west-1:111122223333:instance/i-deadbeef".to_string(),
            tags: vec![("Team".to_string(), "alpha".to_string())],
        }];
        let associator = Associator::new(&[re], &resources);
        let matched = associator.associate_metric_to_resource(
            "AWS/EC2",
            &[("InstanceId".to_string(), "i-deadbeef".to_string())],
        );
        assert!(matched.is_some());
        assert_eq!(
            matched.unwrap().tags,
            vec![("Team".to_string(), "alpha".to_string())]
        );
    }

    #[test]
    fn amazonmq_broker_strips_trailing_numeric_suffix_for_match() {
        let re = Regex::new(r"broker:(?P<Broker>[^:]+)").unwrap();
        let resources = vec![TaggedResource {
            arn: "arn:aws:mq:us-east-1:111122223333:broker:mybroker:abc-179".to_string(),
            tags: vec![("CostCenter".to_string(), "42".to_string())],
        }];
        let associator = Associator::new(&[re], &resources);
        let matched = associator.associate_metric_to_resource(
            "AWS/AmazonMQ",
            &[("Broker".to_string(), "mybroker-1".to_string())],
        );
        assert!(matched.is_some());
        assert_eq!(
            matched.unwrap().tags,
            vec![("CostCenter".to_string(), "42".to_string())]
        );
    }

    #[test]
    fn apply_custom_metadata_labels_appends_string_attributes() {
        let mut dp = SummaryDataPoint::default();
        let meta = HashMap::from([("env".to_string(), "staging".to_string())]);
        apply_custom_metadata_labels(&mut dp, &meta);
        assert!(
            dp.attributes
                .iter()
                .any(|kv| kv.key == "env" && attribute_string_value(kv) == "staging")
        );
    }

    #[tokio::test]
    async fn enrich_unsupported_namespace_skips_rgt_but_adds_custom_metadata() {
        let aws_cfg = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region("us-east-1")
            .load()
            .await;
        let client = RgtClient::new(&aws_cfg);
        let mut dp = SummaryDataPoint {
            attributes: vec![
                string_attr("MetricName", "CPUUtilization"),
                string_attr("Namespace", "AWS/NotInYaceRegistry"),
            ],
            ..Default::default()
        };
        let mut cache = NamespaceResourceCache::default();
        let meta = HashMap::from([("static".to_string(), "yes".to_string())]);
        enrich_summary_datapoint(
            &client,
            &mut dp,
            &mut cache,
            false,
            "/tmp",
            std::time::Duration::from_secs(3600),
            true,
            &meta,
        )
        .await
        .unwrap();
        assert!(
            dp.attributes
                .iter()
                .any(|kv| kv.key == "static" && attribute_string_value(kv) == "yes")
        );
    }
}
