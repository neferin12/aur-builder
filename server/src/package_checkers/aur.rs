use std::error::Error;
use serde::Deserialize;
use common::types::{AurPackageSettings, PackageSearchResult};
use common::errors::MissingFieldError;

#[derive(Deserialize)]
pub struct AurResponse {
    pub results: Vec<AurPackageInfo>,
}

#[derive(Deserialize)]
pub struct AurPackageInfo {
    #[serde(rename = "Name")]
    pub name: String,

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
pub async fn get_aur_data(package: &AurPackageSettings) -> Result<PackageSearchResult, Box<dyn Error>> {
    let url = format!("https://aur.archlinux.org/rpc/v5/info?arg[]={}", package.name);
    let resp = reqwest::get(&url).await?.json::<AurResponse>().await?;

    let package_info = resp.results.get(0).ok_or_else(|| MissingFieldError::new("results".to_string()))?;

    let result = PackageSearchResult {
        name: package_info.name.clone(),
        version: package_info.version.clone(),
        maintainer: package_info.maintainer.clone().unwrap_or_default(),
        last_modified: package_info.last_modified,
        source: None,
        subfolder: None,
        options: package.options.clone(),
        environment: package.env.clone(),
    };
    
    Ok(result)
}