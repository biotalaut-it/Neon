use age::{secrecy::SecretString, Decryptor, Encryptor};
use rusqlite::{backup::Backup, Connection, DatabaseName};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    iter,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_DATABASE_BYTES: u64 = 512 * 1024 * 1024;

pub struct PrivacyManager;
impl PrivacyManager {
    pub fn backup_encrypted(
        database: &Path,
        archive: &Path,
        passphrase: &str,
    ) -> Result<(), String> {
        validate_passphrase(passphrase)?;
        if let Some(parent) = archive.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if same_target(database, archive)? {
            return Err("backup archive must not overwrite the database".into());
        }
        let metadata = fs::metadata(database).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_DATABASE_BYTES {
            return Err("database exceeds the 512 MiB encrypted backup limit".into());
        }
        let temporary = unique_sibling(archive, "encrypted")?;
        match encrypt_database(database, &temporary, passphrase) {
            Ok(()) => fs::rename(&temporary, archive)
                .map_err(|error| format!("cannot install encrypted backup: {error}")),
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(error)
            }
        }
    }

    pub fn restore_encrypted(
        archive: &Path,
        database: &Path,
        passphrase: &str,
        replace_existing: bool,
    ) -> Result<(), String> {
        validate_passphrase(passphrase)?;
        if let Some(parent) = database.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if same_target(archive, database)? {
            return Err("restore archive and database target must be different files".into());
        }
        if database.exists() && !replace_existing {
            return Err("restore target exists; explicit replacement approval is required".into());
        }
        let temporary = unique_sibling(database, "restore")?;
        if let Err(error) = decrypt_database(archive, &temporary, passphrase) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let installation = (|| {
            if database.exists() && !replace_existing {
                return Err("restore target appeared during restore; refusing replacement".into());
            }
            if database.exists() {
                let current = Connection::open(database).map_err(|error| {
                    format!("cannot open current database before restore: {error}")
                })?;
                current
                    .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                    .map_err(|error| {
                        format!("cannot checkpoint current database before restore: {error}")
                    })?;
                drop(current);
                for sidecar in [suffix_path(database, "-wal"), suffix_path(database, "-shm")] {
                    match fs::remove_file(&sidecar) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(format!(
                                "cannot clear old SQLite sidecar {}: {error}",
                                sidecar.display()
                            ))
                        }
                    }
                }
            }
            fs::rename(&temporary, database)
                .map_err(|error| format!("cannot install restored database: {error}"))
        })();
        if installation.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        installation
    }

    pub fn purge_local_database(database: &Path, confirmed: bool) -> Result<(), String> {
        if !confirmed {
            return Err("local data purge requires explicit confirmation".into());
        }
        if database.exists() {
            let connection = Connection::open(database).map_err(|error| error.to_string())?;
            connection
                .execute_batch("PRAGMA secure_delete=ON; PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(|error| format!("cannot checkpoint local database: {error}"))?;
            drop(connection);
        }
        for path in [
            database.to_path_buf(),
            suffix_path(database, "-wal"),
            suffix_path(database, "-shm"),
        ] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("cannot purge {}: {error}", path.display())),
            }
        }
        Ok(())
    }
}

fn encrypt_database(database: &Path, archive: &Path, passphrase: &str) -> Result<(), String> {
    let source = Connection::open(database).map_err(|error| error.to_string())?;
    let mut snapshot = Connection::open_in_memory().map_err(|error| error.to_string())?;
    let backup = Backup::new(&source, &mut snapshot)
        .map_err(|error| format!("cannot create SQLite snapshot: {error}"))?;
    backup
        .run_to_completion(64, std::time::Duration::from_millis(10), None)
        .map_err(|error| format!("cannot finish SQLite snapshot: {error}"))?;
    drop(backup);
    let serialized = snapshot
        .serialize(DatabaseName::Main)
        .map_err(|error| format!("cannot serialize SQLite snapshot: {error}"))?;
    if serialized.len() as u64 > MAX_DATABASE_BYTES {
        return Err("database snapshot exceeds the 512 MiB limit".into());
    }
    let output = private_create_new(archive)?;
    let encryptor =
        Encryptor::with_user_passphrase(SecretString::new(passphrase.to_owned().into()));
    let mut writer = encryptor
        .wrap_output(output)
        .map_err(|error| format!("cannot initialize encrypted backup: {error}"))?;
    writer
        .write_all(&serialized)
        .map_err(|error| format!("cannot write encrypted backup: {error}"))?;
    writer
        .finish()
        .map_err(|error| format!("cannot finalize encrypted backup: {error}"))?;
    Ok(())
}

fn decrypt_database(archive: &Path, destination: &Path, passphrase: &str) -> Result<(), String> {
    let input =
        File::open(archive).map_err(|error| format!("cannot open encrypted backup: {error}"))?;
    let decryptor =
        Decryptor::new(input).map_err(|error| format!("invalid encrypted backup: {error}"))?;
    let secret = SecretString::new(passphrase.to_owned().into());
    if !decryptor.is_scrypt() {
        return Err("backup is not passphrase-encrypted".into());
    }
    let identity = age::scrypt::Identity::new(secret);
    let mut reader = decryptor
        .decrypt(iter::once(&identity as _))
        .map_err(|error| format!("cannot decrypt backup: {error}"))?;
    let mut output = private_create_new(destination)?;
    let copied = io::copy(
        &mut reader.by_ref().take(MAX_DATABASE_BYTES + 1),
        &mut output,
    )
    .map_err(|error| format!("cannot decrypt backup contents: {error}"))?;
    if copied > MAX_DATABASE_BYTES {
        return Err("decrypted database exceeds the 512 MiB restore limit".into());
    }
    output.sync_all().map_err(|error| error.to_string())?;
    drop(output);
    let restored = Connection::open(destination)
        .map_err(|error| format!("decrypted data is not a SQLite database: {error}"))?;
    let integrity: String = restored
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if integrity != "ok" {
        return Err(format!(
            "restored database failed integrity check: {integrity}"
        ));
    }
    restored
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|error| error.to_string())
}

fn private_create_new(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("cannot create private file {}: {error}", path.display()))
}

fn unique_sibling(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .ok_or("target path must have a file name")?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    Ok(parent.join(format!(
        ".{name}.neon-{suffix}-{}-{nonce}",
        std::process::id()
    )))
}

fn suffix_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

fn same_target(source: &Path, destination: &Path) -> Result<bool, String> {
    let source = source.canonicalize().map_err(|error| error.to_string())?;
    let destination = if destination.exists() {
        destination
            .canonicalize()
            .map_err(|error| error.to_string())?
    } else {
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        parent
            .canonicalize()
            .map_err(|error| error.to_string())?
            .join(
                destination
                    .file_name()
                    .ok_or("target path must have a file name")?,
            )
    };
    Ok(source == destination)
}

fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if !(12..=4096).contains(&passphrase.len()) {
        return Err("backup passphrase must contain 12..=4096 bytes".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PrivacyManager;
    use rusqlite::Connection;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "neon-privacy-{label}-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn encrypted_backup_roundtrips_and_wrong_passphrase_keeps_target_absent() {
        let database = temp_path("source");
        let archive = temp_path("archive");
        let restored = temp_path("restored");
        let source = Connection::open(&database).unwrap();
        source
            .execute_batch(
                "CREATE TABLE secrets(value TEXT); INSERT INTO secrets VALUES('private-data');",
            )
            .unwrap();
        drop(source);

        PrivacyManager::backup_encrypted(&database, &archive, "correct horse battery staple")
            .unwrap();
        assert!(!fs::read(&archive)
            .unwrap()
            .windows(b"private-data".len())
            .any(|window| window == b"private-data"));
        assert!(PrivacyManager::restore_encrypted(
            &archive,
            &restored,
            "incorrect horse battery staple",
            false,
        )
        .is_err());
        assert!(!restored.exists());
        PrivacyManager::restore_encrypted(
            &archive,
            &restored,
            "correct horse battery staple",
            false,
        )
        .unwrap();
        let restored_db = Connection::open(&restored).unwrap();
        let value: String = restored_db
            .query_row("SELECT value FROM secrets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "private-data");
        drop(restored_db);
        let _ = fs::remove_file(database);
        let _ = fs::remove_file(archive);
        let _ = fs::remove_file(restored);
    }

    #[test]
    fn local_purge_requires_confirmation_and_removes_database() {
        let database = temp_path("purge");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch("CREATE TABLE local_data(value TEXT);")
            .unwrap();
        drop(connection);
        assert!(PrivacyManager::purge_local_database(&database, false).is_err());
        PrivacyManager::purge_local_database(&database, true).unwrap();
        assert!(!database.exists());
    }
}
