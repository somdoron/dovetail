use anyhow::{Context, Result, bail, ensure};
use oci_client::{Reference, secrets::RegistryAuth};
use serde_json::Value;
use std::collections::BTreeMap;

pub fn credential_server(reference: &Reference) -> &str {
    match reference.registry() {
        "docker.io" | "index.docker.io" | "registry-1.docker.io" => "https://index.docker.io/v1/",
        registry => registry,
    }
}
fn credentials(reference: &Reference) -> Result<Option<docker_credential::DockerCredential>> {
    let directory = std::env::var_os("DOCKER_CONFIG")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".docker"))
        });
    let Some(directory) = directory else {
        return Ok(None);
    };
    let content = match std::fs::read(directory.join("config.json")) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("reading Docker credential configuration"),
    };
    from_config(reference, &content)
}

pub(super) fn from_config(
    reference: &Reference,
    content: &[u8],
) -> Result<Option<docker_credential::DockerCredential>> {
    let mut config: Value =
        serde_json::from_slice(content).context("invalid Docker credential configuration")?;
    // Docker selects helpers/stores before inline auth. The crate checks inline
    // auth before credsStore, so remove inline entries when a store is configured.
    if config
        .get("credsStore")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
    {
        config["auths"] = serde_json::json!({});
    } else if let Some(object) = config.as_object_mut() {
        object.remove("credsStore");
    }
    let result = docker_credential::get_credential_from_reader(
        std::io::Cursor::new(serde_json::to_vec(&config)?),
        credential_server(reference),
    );
    match result {
        Ok(credential) => Ok(Some(credential)),
        Err(docker_credential::CredentialRetrievalError::NoCredentialConfigured) => Ok(None),
        Err(docker_credential::CredentialRetrievalError::HelperFailure { stdout, .. })
            if stdout.trim() == "credentials not found in native keychain" =>
        {
            Ok(None)
        }
        // Helper errors can include stdout containing secrets. Never format them.
        Err(_) => bail!(
            "cannot retrieve Docker credentials for {}; check docker login and the configured credential helper",
            reference.registry()
        ),
    }
}
pub async fn resolve(reference: &Reference, push: bool) -> Result<RegistryAuth> {
    match credentials(reference)? {
        Some(docker_credential::DockerCredential::UsernamePassword(user, password)) => {
            Ok(RegistryAuth::Basic(user, password))
        }
        Some(docker_credential::DockerCredential::IdentityToken(token)) => {
            exchange_identity(reference, &token, push).await
        }
        None if !push => Ok(RegistryAuth::Anonymous),
        None => bail!(
            "no Docker credentials for {}; run docker login {} first",
            reference.registry(),
            reference.registry()
        ),
    }
}
async fn exchange_identity(
    reference: &Reference,
    refresh_token: &str,
    push: bool,
) -> Result<RegistryAuth> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("https://{}/v2/", reference.resolve_registry()))
        .send()
        .await?;
    let header = response
        .headers()
        .get("www-authenticate")
        .context("registry did not advertise token authentication")?
        .to_str()?;
    let challenge = http_auth::ChallengeParser::new(header)
        .filter_map(Result::ok)
        .find(|challenge| challenge.scheme.eq_ignore_ascii_case("bearer"))
        .context("registry has no Bearer challenge")?;
    let mut parameters = BTreeMap::new();
    for (name, value) in challenge.params {
        parameters.insert(name.to_ascii_lowercase(), value.to_unescaped());
    }
    let realm = parameters
        .get("realm")
        .context("token challenge has no realm")?;
    ensure!(
        reqwest::Url::parse(realm)?.scheme() == "https",
        "registry token endpoint must use HTTPS"
    );
    let scope = format!(
        "repository:{}:{}",
        reference.repository(),
        if push { "pull,push" } else { "pull" }
    );
    let service = parameters.get("service").map(String::as_str).unwrap_or("");
    let response = client
        .post(realm)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("service", service),
            ("scope", &scope),
            ("client_id", "dovetail"),
        ])
        .send()
        .await?
        .error_for_status()?;
    let body: Value = response.json().await?;
    let token = body
        .get("access_token")
        .or_else(|| body.get("token"))
        .and_then(Value::as_str)
        .context("registry token response has no access token")?;
    Ok(RegistryAuth::Bearer(token.into()))
}
