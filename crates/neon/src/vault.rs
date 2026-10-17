use age::{secrecy::SecretString, Decryptor, Encryptor};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    io::{Read, Write},
    iter,
    path::Path,
};
use zeroize::{Zeroize, Zeroizing};

pub struct SecretVault {
    conn: Connection,
}
impl SecretVault {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let existed = path.exists();
        let conn = Connection::open(path).map_err(|error| error.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS encrypted_secrets(
                name TEXT PRIMARY KEY,
                ciphertext BLOB NOT NULL,
                updated_at INTEGER NOT NULL
            );",
        )
        .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        if !existed {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|error| format!("cannot secure secret vault permissions: {error}"))?;
        }
        Ok(Self { conn })
    }

    pub fn set(&mut self, name: &str, value: &str, passphrase: &str) -> Result<(), String> {
        validate_name(name)?;
        validate_passphrase(passphrase)?;
        if value.is_empty() || value.len() > 4096 {
            return Err("secret value must contain 1..=4096 bytes".into());
        }
        let mut ciphertext = Vec::new();
        let encryptor =
            Encryptor::with_user_passphrase(SecretString::new(passphrase.to_owned().into()));
        let mut writer = encryptor
            .wrap_output(&mut ciphertext)
            .map_err(|error| format!("cannot initialize secret encryption: {error}"))?;
        writer
            .write_all(value.as_bytes())
            .map_err(|error| format!("cannot encrypt secret: {error}"))?;
        writer
            .finish()
            .map_err(|error| format!("cannot finalize secret encryption: {error}"))?;
        self.conn
            .execute(
                "INSERT INTO encrypted_secrets(name,ciphertext,updated_at) VALUES(?1,?2,unixepoch()) ON CONFLICT(name) DO UPDATE SET ciphertext=excluded.ciphertext,updated_at=excluded.updated_at",
                params![name, ciphertext],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn get(&self, name: &str, passphrase: &str) -> Result<Option<Zeroizing<String>>, String> {
        validate_name(name)?;
        validate_passphrase(passphrase)?;
        let ciphertext: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT ciphertext FROM encrypted_secrets WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(ciphertext) = ciphertext else {
            return Ok(None);
        };
        let decryptor = Decryptor::new(ciphertext.as_slice())
            .map_err(|error| format!("invalid encrypted secret: {error}"))?;
        if !decryptor.is_scrypt() {
            return Err("secret is not passphrase-encrypted".into());
        }
        let identity = age::scrypt::Identity::new(SecretString::new(passphrase.to_owned().into()));
        let mut reader = decryptor
            .decrypt(iter::once(&identity as _))
            .map_err(|error| format!("cannot decrypt secret: {error}"))?;
        let mut plaintext = Vec::new();
        reader
            .by_ref()
            .take(4097)
            .read_to_end(&mut plaintext)
            .map_err(|error| format!("cannot read decrypted secret: {error}"))?;
        if plaintext.is_empty() || plaintext.len() > 4096 {
            plaintext.zeroize();
            return Err("decrypted secret exceeds its size limit".into());
        }
        let secret =
            String::from_utf8(plaintext).map_err(|_| "decrypted secret is not valid UTF-8")?;
        Ok(Some(Zeroizing::new(secret)))
    }

    pub fn list(&self) -> Result<Vec<String>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT name FROM encrypted_secrets ORDER BY name")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn delete(&mut self, name: &str, confirmed: bool) -> Result<(), String> {
        validate_name(name)?;
        if !confirmed {
            return Err("secret deletion requires explicit confirmation".into());
        }
        self.conn
            .execute_batch("PRAGMA secure_delete=ON;")
            .map_err(|error| error.to_string())?;
        let changed = self
            .conn
            .execute("DELETE FROM encrypted_secrets WHERE name=?1", [name])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("secret name not found".into());
        }
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("secret name must use 1..=128 ASCII letters, digits, '.', '_' or '-'".into());
    }
    Ok(())
}

fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if !(12..=4096).contains(&passphrase.len()) {
        return Err("vault passphrase must contain 12..=4096 bytes".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SecretVault;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "neon-vault-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn vault_encrypts_values_and_requires_confirmation_to_delete() {
        let path = temp_path();
        let mut vault = SecretVault::open(&path).unwrap();
        vault
            .set(
                "remote-api-key",
                "token-not-in-cleartext",
                "correct horse battery staple",
            )
            .unwrap();
        assert!(!fs::read(&path)
            .unwrap()
            .windows(b"token-not-in-cleartext".len())
            .any(|window| window == b"token-not-in-cleartext"));
        assert!(vault
            .get("remote-api-key", "incorrect horse battery staple")
            .is_err());
        assert_eq!(
            vault
                .get("remote-api-key", "correct horse battery staple")
                .unwrap()
                .unwrap()
                .as_str(),
            "token-not-in-cleartext"
        );
        assert!(vault.delete("remote-api-key", false).is_err());
        vault.delete("remote-api-key", true).unwrap();
        assert!(vault.list().unwrap().is_empty());
        drop(vault);
        let _ = fs::remove_file(path);
    }
}
