use std::error::Error;
use serde::Deserialize;
use common::types::{AurPackageSettings, PackageSearchResult};
use common::errors::{AurRequestError, MissingFieldError};

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
    get_aur_data_with_base_url(package, "https://aur.archlinux.org/rpc/v5").await
}

pub async fn get_aur_data_with_base_url(package: &AurPackageSettings, base_url: &str) -> Result<PackageSearchResult, Box<dyn Error>> {
    let url = format!("{}/info?arg[]={}", base_url, package.name);
    let resp = reqwest::get(&url).await?;
    if !resp.status().is_success() {
        return Err(AurRequestError::new(package.name.clone(), resp.status().as_u16()).into())
    }
    let resp_json = resp.json::<AurResponse>().await?;

    let package_info = resp_json.results.get(0).ok_or_else(|| MissingFieldError::new("results".to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use common::types::AurPackageSettings;

    #[test]
    fn test_aur_response_deserialization() {
        let json = r#"{"results": [{"Name": "test-pkg", "Version": "1.2.3-1", "Maintainer": "testuser", "LastModified": 1700000000}]}"#;
        let resp: AurResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results.len(), 1);
        assert_eq!(resp.results[0].name, "test-pkg");
        assert_eq!(resp.results[0].version, "1.2.3-1");
        assert_eq!(resp.results[0].maintainer, Some("testuser".to_string()));
        assert_eq!(resp.results[0].last_modified, 1700000000);
    }

    #[test]
    fn test_aur_response_deserialization_null_maintainer() {
        let json = r#"{"results": [{"Name": "orphan-pkg", "Version": "0.1-1", "Maintainer": null, "LastModified": 1600000000}]}"#;
        let resp: AurResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results[0].maintainer, None);
    }

    #[test]
    fn test_aur_response_empty_results() {
        let json = r#"{"results": []}"#;
        let resp: AurResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.results.len(), 0);
    }

    #[test]
    fn test_aur_package_info_fields() {
        let json = r#"{"Name": "mypkg", "Version": "5.0-2", "Maintainer": "dev", "LastModified": 9999}"#;
        let info: AurPackageInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.name, "mypkg");
        assert_eq!(info.version, "5.0-2");
        assert_eq!(info.maintainer, Some("dev".to_string()));
        assert_eq!(info.last_modified, 9999);
    }

    #[tokio::test]
    async fn test_get_aur_data_with_base_url_success() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/info?arg[]=test-package")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"results": [{"Name": "test-package", "Version": "1.0.0-1", "Maintainer": "maintainer", "LastModified": 1700000000}]}"#,
            )
            .create_async()
            .await;

        let pkg = AurPackageSettings {
            name: "test-package".to_string(),
            env: None,
            options: None,
        };
        let result = get_aur_data_with_base_url(&pkg, &server.url()).await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data.name, "test-package");
        assert_eq!(data.version, "1.0.0-1");
        assert_eq!(data.maintainer, "maintainer");
        assert_eq!(data.last_modified, 1700000000);
        assert!(data.source.is_none());
    }

    #[tokio::test]
    async fn test_get_aur_data_with_base_url_null_maintainer_defaults_to_empty() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/info?arg[]=orphan-pkg")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"results": [{"Name": "orphan-pkg", "Version": "2.0-1", "Maintainer": null, "LastModified": 1600000000}]}"#,
            )
            .create_async()
            .await;

        let pkg = AurPackageSettings {
            name: "orphan-pkg".to_string(),
            env: None,
            options: None,
        };
        let result = get_aur_data_with_base_url(&pkg, &server.url()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().maintainer, "");
    }

    #[tokio::test]
    async fn test_get_aur_data_with_base_url_http_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/info?arg[]=bad-pkg")
            .with_status(404)
            .create_async()
            .await;

        let pkg = AurPackageSettings {
            name: "bad-pkg".to_string(),
            env: None,
            options: None,
        };
        let result = get_aur_data_with_base_url(&pkg, &server.url()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bad-pkg"));
    }

    #[tokio::test]
    async fn test_get_aur_data_with_base_url_empty_results() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/info?arg[]=unknown-pkg")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"results": []}"#)
            .create_async()
            .await;

        let pkg = AurPackageSettings {
            name: "unknown-pkg".to_string(),
            env: None,
            options: None,
        };
        let result = get_aur_data_with_base_url(&pkg, &server.url()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("results"));
    }

    #[tokio::test]
    async fn test_get_aur_data_with_base_url_preserves_options() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/info?arg[]=my-pkg")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"results": [{"Name": "my-pkg", "Version": "1.0-1", "Maintainer": "dev", "LastModified": 100}]}"#,
            )
            .create_async()
            .await;

        let pkg = AurPackageSettings {
            name: "my-pkg".to_string(),
            env: None,
            options: Some("--sign".to_string()),
        };
        let result = get_aur_data_with_base_url(&pkg, &server.url()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().options, Some("--sign".to_string()));
    }
}