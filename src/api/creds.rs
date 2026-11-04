use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StoredCreds {
    pub subdomain: String,
    pub library_id: Option<i64>,
    pub library_name: Option<String>,
    #[serde(default)]
    pub card_number: String,
    #[serde(default)]
    pub pin: String,
    #[serde(default)]
    pub bootstrap_access_token: Option<String>,
    #[serde(default)]
    pub bootstrap_expires_at_unix: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub authentication_token: Option<String>,
}

impl StoredCreds {
    pub fn has_card_and_pin(&self) -> bool {
        !self.card_number.is_empty() && !self.pin.is_empty()
    }
}

pub fn creds_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("no config dir (XDG_CONFIG_HOME/HOME unset)")?
        .join("freegal")
        .join("creds.toml"))
}

pub fn load(path: &std::path::Path) -> Result<Option<StoredCreds>> {
    match fs::read(path) {
        Ok(bytes) => {
            let creds =
                toml::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
            Ok(Some(creds))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

pub fn save(path: &std::path::Path, creds: &StoredCreds) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.exists() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
    }
    let tmp = path.with_extension("toml.tmp");
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("creating {}", tmp.display()))?;
        let contents = toml::to_string(creds).context("serializing creds")?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("committing {}", path.display()))?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_load_roundtrip_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("creds.toml");
        let creds = StoredCreds {
            subdomain: "demo".into(),
            library_id: Some(404),
            library_name: Some("Demo Public Library".into()),
            card_number: "12345678".into(),
            pin: "9999".into(),
            bootstrap_access_token: Some("BOOT".into()),
            bootstrap_expires_at_unix: Some(1_900_000_000),
            refresh_token: Some("REF".into()),
            authentication_token: Some("SESS".into()),
        };
        save(&path, &creds).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file must be 0600, got {mode:o}");
        let loaded = load(&path).unwrap().unwrap();
        assert_eq!(loaded, creds);
    }

    #[test]
    fn load_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("nope.toml")).unwrap().is_none());
    }

    #[test]
    fn directory_created_0700() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b");
        let path = nested.join("creds.toml");
        save(&path, &StoredCreds::default()).unwrap();
        let mode = fs::metadata(&nested).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "creds dir must be 0700, got {mode:o}");
    }
}
