use std::fmt::Debug;
use std::num::ParseIntError;
use std::str::FromStr;
use std::string::String;
use std::{env, fmt};

use aws_config::SdkConfig;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
// use cx_sdk_rest_logs::auth::ApiKey;


// let api_key = env::var("CORALOGIX_API_KEY").unwrap_or_default();
// let endpoint = env::var("CORALOGIX_DOMAIN").unwrap_or_else(|_| "ingress.private.coralogix.com".to_string());
// let app_name = env::var("APP_NAME").unwrap_or_else(|_| "aws".to_string());
// let sub_name = env::var("SUB_NAME").unwrap_or_else(|_| "metrics".to_string());
// let retry_limit: usize = env::var("RETRY_LIMIT").unwrap_or_else(|_| "3".to_string()).parse().unwrap_or(3);
// let retry_delay: u64 = env::var("RETRY_DELAY").unwrap_or_else(|_| "5".to_string()).parse().unwrap_or(5);


pub struct Config {
    pub api_key: String,
    pub endpoint: String,
    pub app_name: String,
    pub sub_name: String,
    pub retry_limit: usize,
    pub retry_delay: u64,
}

impl Config {
    pub fn load_from_env() -> Result<Config, String> {
        let api_key  = env::var("CORALOGIX_API_KEY").map_err(|e| format!("CORALOGIX_API_KEY is not set: {}", e))?;
        let endpoint = env::var("CORALOGIX_DOMAIN").unwrap_or_else(|_| "ingress.private.coralogix.com".to_string());
        let app_name = env::var("APP_NAME").unwrap_or_else(|_| "aws".to_string());
        let sub_name = env::var("SUB_NAME").unwrap_or_else(|_| "metrics".to_string());
        let retry_limit: usize = env::var("RETRY_LIMIT").unwrap_or_else(|_| "3".to_string()).parse().unwrap_or(3);
        let retry_delay: u64 = env::var("RETRY_DELAY").unwrap_or_else(|_| "5".to_string()).parse().unwrap_or(5);

        Ok(Config {
            api_key,
            endpoint,
            app_name,
            sub_name,
            retry_limit,
            retry_delay,
        })
    }
}