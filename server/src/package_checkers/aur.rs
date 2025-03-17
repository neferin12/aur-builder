use common::types::{AurPackageSettings, PackageSearchResult};
use reqwest::Error;

pub type AurResult = Result<PackageSearchResult, Error>;

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
pub async fn get_aur_data(package: &AurPackageSettings) -> AurResult {
    let url = format!("https://aur.archlinux.org/rpc/v5/info?arg[]={}", package.name);
    let resp = reqwest::get(url).await;
    if resp.is_err() {
        return Err(resp.err().unwrap());
    };
    let data: serde_json::Value = resp?.json().await?;
    let results: PackageSearchResult = PackageSearchResult {
        name: package.name.clone(),
        version: String::from(data["results"][0]["Version"].as_str().unwrap()),
        maintainer: String::from(data["results"][0]["Maintainer"].as_str().unwrap()),
        last_modified: data["results"][0]["LastModified"].as_i64().unwrap(),
        source: None,
        subfolder: None,
        options: package.options.clone(),
        environment: package.env.clone()
    };
    Ok(results)
}