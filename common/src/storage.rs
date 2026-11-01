//! Minimal S3 SigV4 query signing. URLs are capabilities; never log them.
use crate::types::{BuildTaskTransmissionFormat, UploadGrant};
use hmac::{Hmac, Mac};
use sea_orm::sqlx::types::chrono::Utc;
use sha2::{Digest, Sha256};
pub type Error = Box<dyn std::error::Error + Send + Sync>;
#[derive(Clone)]
pub struct Storage {
    endpoint: String,
    public_endpoint: String,
    bucket: String,
    region: String,
    access: String,
    secret: String,
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn mac(key: &[u8], value: &str) -> Vec<u8> {
    let mut m = Hmac::<Sha256>::new_from_slice(key).unwrap();
    m.update(value.as_bytes());
    m.finalize().into_bytes().to_vec()
}
fn validate_endpoint(endpoint: &str) -> Result<(), Error> {
    let u = url::Url::parse(endpoint)?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || u.path() != "/"
        || u.query().is_some()
        || u.fragment().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return Err("S3 endpoints must be HTTP origins".into());
    }
    Ok(())
}
impl Storage {
    pub fn from_env() -> Result<Self, Error> {
        let get = |k| std::env::var(k);
        let endpoint = get("AB_S3_ENDPOINT")?;
        let s = Self {
            public_endpoint: get("AB_S3_PUBLIC_ENDPOINT").unwrap_or(endpoint.clone()),
            endpoint,
            bucket: get("AB_S3_BUCKET")?,
            region: get("AB_S3_REGION").unwrap_or("garage".into()),
            access: get("AB_S3_ACCESS_KEY")?,
            secret: get("AB_S3_SECRET_KEY")?,
        };
        for e in [&s.endpoint, &s.public_endpoint] {
            validate_endpoint(e)?;
        }
        Ok(s)
    }
    pub fn sign(
        &self,
        method: &str,
        key: &str,
        seconds: u64,
        public: bool,
    ) -> Result<String, Error> {
        if !(1..=604800).contains(&seconds) {
            return Err("invalid signing lifetime".into());
        }
        let endpoint = if public {
            &self.public_endpoint
        } else {
            &self.endpoint
        };
        let u = url::Url::parse(endpoint)?;
        let host = match u.port() {
            Some(p) => format!("{}:{p}", u.host_str().ok_or("missing host")?),
            None => u.host_str().ok_or("missing host")?.to_string(),
        };
        let now = Utc::now();
        let date = now.format("%Y%m%d").to_string();
        let timestamp = now.format("%Y%m%dT%H%M%SZ").to_string();
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let path = format!(
            "/{}/{}",
            encode(&self.bucket),
            key.split('/').map(encode).collect::<Vec<_>>().join("/")
        );
        let mut q = vec![
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            ("X-Amz-Credential", format!("{}/{}", self.access, scope)),
            ("X-Amz-Date", timestamp.clone()),
            ("X-Amz-Expires", seconds.to_string()),
            ("X-Amz-SignedHeaders", "host".into()),
        ];
        q.sort();
        let query = q
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        let canonical = format!("{method}\n{path}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
        let to_sign = format!(
            "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
            hash(&canonical)
        );
        let k = mac(format!("AWS4{}", self.secret).as_bytes(), &date);
        let k = mac(&k, &self.region);
        let k = mac(&k, "s3");
        let k = mac(&k, "aws4_request");
        Ok(format!(
            "{}{path}?{query}&X-Amz-Signature={}",
            u.origin().ascii_serialization(),
            hex::encode(mac(&k, &to_sign))
        ))
    }
    pub fn grant(&self, task: &mut BuildTaskTransmissionFormat) -> Result<(), Error> {
        task.attempt = uuid::Uuid::new_v4().to_string();
        task.object_key = format!("builds/{}/bundle.tar", task.attempt);
        self.renew(task)
    }
    pub fn renew(&self, task: &mut BuildTaskTransmissionFormat) -> Result<(), Error> {
        let ttl = std::env::var("AB_UPLOAD_TTL")
            .unwrap_or("86400".into())
            .parse::<u64>()?;
        task.upload = Some(UploadGrant {
            url: self.sign("PUT", &task.object_key, ttl, true)?,
            expires_at: Utc::now().timestamp() + ttl as i64,
        });
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_validation_and_normalization() {
        for endpoint in [
            "http://:password@example.com",
            "https://example.com/#fragment",
            "https://example.com/prefix",
            "ftp://example.com",
            "https://example.com/?query",
        ] {
            assert!(validate_endpoint(endpoint).is_err());
        }
        let storage = Storage {
            endpoint: "http://LOCALHOST:80/".into(),
            public_endpoint: "http://LOCALHOST:80/".into(),
            bucket: "private".into(),
            region: "garage".into(),
            access: "test".into(),
            secret: "secret".into(),
        };
        assert!(
            storage
                .sign("GET", "file", 60, true)
                .unwrap()
                .starts_with("http://localhost/private/file?")
        );
    }
    #[test]
    fn encoding() {
        assert_eq!(encode("a +/é"), "a%20%2B%2F%C3%A9");
    }
}
