use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::{
    error::Result,
    model::{PublicJwk, StoredActivation},
};

const BUILTIN_JWK: &str = r#"{"kty":"OKP","crv":"Ed25519","x":"SdQb9d4-MW-rM91EUUrHEnVhv3-MfyymX0o_cWc3UXk","kid":"2026.08.05","alg":"EdDSA","use":"sig"}"#;

#[derive(Clone)]
pub struct Storage {
    path: PathBuf,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let storage = Self {
            path: path.as_ref().to_owned(),
        };
        storage.with_connection(|connection| {
            connection.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA foreign_keys=ON;
                 CREATE TABLE IF NOT EXISTS signing_keys (
                    kid TEXT PRIMARY KEY,
                    jwk_json TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS activation (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    activation_json TEXT NOT NULL
                 );",
            )?;
            Ok(())
        })?;
        Ok(storage)
    }

    pub fn seed_builtin_key(&self) -> Result<()> {
        let key: PublicJwk = serde_json::from_str(BUILTIN_JWK)?;
        self.save_keys(&[key])
    }

    pub fn save_keys(&self, keys: &[PublicJwk]) -> Result<()> {
        self.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            for key in keys {
                transaction.execute(
                    "INSERT INTO signing_keys (kid, jwk_json, updated_at)
                     VALUES (?1, ?2, unixepoch())
                     ON CONFLICT(kid) DO UPDATE SET
                       jwk_json = excluded.jwk_json,
                       updated_at = excluded.updated_at",
                    params![key.kid, serde_json::to_string(key)?],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn load_keys(&self) -> Result<Vec<PublicJwk>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare("SELECT jwk_json FROM signing_keys")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            let mut keys = Vec::new();
            for row in rows {
                keys.push(serde_json::from_str(&row?)?);
            }
            Ok(keys)
        })
    }

    pub fn save_activation(&self, activation: &StoredActivation) -> Result<()> {
        let json = serde_json::to_string(activation)?;
        self.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO activation (id, activation_json) VALUES (1, ?1)
                 ON CONFLICT(id) DO UPDATE SET activation_json = excluded.activation_json",
                [json],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn load_activation(&self) -> Result<Option<StoredActivation>> {
        self.with_connection(|connection| {
            let mut statement =
                connection.prepare("SELECT activation_json FROM activation WHERE id = 1")?;
            let mut rows = statement.query([])?;
            match rows.next()? {
                Some(row) => {
                    let json: String = row.get(0)?;
                    Ok(Some(serde_json::from_str(&json)?))
                }
                None => Ok(None),
            }
        })
    }

    pub fn clear_activation(&self) -> Result<()> {
        self.with_connection(|connection| {
            connection.execute("DELETE FROM activation WHERE id = 1", [])?;
            Ok(())
        })
    }

    fn with_connection<T>(&self, operation: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        operation(&connection)
    }
}

#[cfg(test)]
mod tests {
    use crate::model::ActivationSource;

    use super::*;

    fn activation(id: &str) -> StoredActivation {
        StoredActivation {
            token: id.into(),
            source: ActivationSource::Offline,
            activated_at: 100,
        }
    }

    #[test]
    fn persists_and_replaces_single_activation() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("test.db")).unwrap();
        storage.seed_builtin_key().unwrap();
        assert!(!storage.load_keys().unwrap().is_empty());
        storage.save_activation(&activation("first")).unwrap();
        storage.save_activation(&activation("second")).unwrap();
        assert_eq!(
            storage.load_activation().unwrap().unwrap().token,
            "second"
        );
        storage.clear_activation().unwrap();
        assert!(storage.load_activation().unwrap().is_none());
    }
}
