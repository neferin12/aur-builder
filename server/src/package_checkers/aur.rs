use common::errors::{AurRequestError, MissingFieldError};
use common::types::{AurPackageSettings, PackageSearchResult};
use serde::Deserialize;
use std::error::Error;

#[derive(Deserialize)]
pub struct AurResponse {
    pub results: Vec<AurPackageInfo>,
}

#[derive(Deserialize)]
pub struct AurPackageInfo {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "PackageBase")]
    pub package_base: String,

    #[serde(rename = "Version")]
    pub version: String,

    #[serde(rename = "Maintainer")]
    pub maintainer: Option<String>,

    #[serde(rename = "LastModified")]
    pub last_modified: i64,
}

/// This asynchronous function fetches data from the Arch User Repository (AUR) for a given package.
///
/// # Arguments
///
/// * `package` - A string slice that holds the name of the package.
///
/// # Returns
///
/// * `AurResult` - A Result type that returns `AurRequestResultStruct` on success, or `Error` on failure.
///
/// # Errors
///
/// This function will return an error if the HTTP request fails or if the JSON parsing fails.
///
/// # Example
///
/// ```
/// let data = get_aur_data("cvc5").await.unwrap();
/// ```
pub async fn get_aur_data(
    package: &AurPackageSettings,
) -> Result<PackageSearchResult, Box<dyn Error>> {
    let url = format!(
        "https://aur.archlinux.org/rpc/v5/info?arg[]={}",
        package.name
    );
    // AUR's Anubis protection rejects requests without a User-Agent with HTML.
    let resp = reqwest::Client::new()
        .get(&url)
        .header(
            reqwest::header::USER_AGENT,
            format!("aur-builder/{}", common::environment::VERSION),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AurRequestError::new(package.name.clone(), resp.status().as_u16()).into());
    }
    let status = resp.status();
    let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok()).unwrap_or("unknown").to_owned();
    let resp_json = resp.json::<AurResponse>().await.map_err(|error| {
        log::error!("AUR metadata decoding failed: package={}, status={}, content_type={:?}, error={:?}", package.name, status, content_type, error);
        error
    })?;

    let package_info = resp_json
        .results
        .get(0)
        .ok_or_else(|| MissingFieldError::new("results".to_string()))?;

    let result = PackageSearchResult {
        name: package_info.name.clone(),
        version: package_info.version.clone(),
        maintainer: package_info.maintainer.clone().unwrap_or_default(),
        last_modified: package_info.last_modified,
        source: Some(format!(
            "https://aur.archlinux.org/{}.git",
            package_info.package_base
        )),
        subfolder: None,
        options: package.options.clone(),
        environment: package.env.clone(),
    };

    Ok(result)
}
