use cx_sdk_rest_logs::auth::ApiKey;
use std::string::String;
use std::env;

pub struct Config {
    pub api_key: ApiKey,
    pub endpoint: String,
    pub app_name: String,
    pub sub_name: String,
    pub retry_limit: usize,
    pub retry_delay: u64,
}

impl Config {
    pub fn load_from_env() -> Result<Config, String> {
        let api_key = env::var("CORALOGIX_API_KEY")
            .map_err(|e| format!("CORALOGIX_API_KEY is not set: {}", e))?
            .into();
        let endpoint = env::var("CORALOGIX_ENDPOINT")
            .unwrap_or_else(|_| "https://ingress.private.coralogix.com".to_string());
        let app_name = env::var("APP_NAME").unwrap_or_else(|_| "aws".to_string());
        let sub_name = env::var("SUB_NAME").unwrap_or_else(|_| "metrics".to_string());
        let retry_limit: usize = env::var("RETRY_LIMIT")
            .unwrap_or_else(|_| "3".to_string())
            .parse()
            .unwrap_or(3);
        let retry_delay: u64 = env::var("RETRY_DELAY")
            .unwrap_or_else(|_| "5".to_string())
            .parse()
            .unwrap_or(5);

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


impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            api_key: self.api_key.clone(),
            endpoint: self.endpoint.clone(),
            app_name: self.app_name.clone(),
            sub_name: self.sub_name.clone(),
            retry_limit: self.retry_limit.clone(),
            retry_delay: self.retry_delay.clone(),
        }
    }
}