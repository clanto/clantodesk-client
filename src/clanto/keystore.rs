// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use hbb_common::{
    anyhow::{anyhow, Context, Result},
    config::Config,
    log,
    password_security::{set_key_provider, KeyProvider, KeyScope},
    rand::{thread_rng, Rng},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Once},
    time::Duration,
};

const KEY_LEN: usize = 32;
const MACHINE_KEY_FILE: &str = "machine.key";
const USER_KEY_FILE: &str = "user.key";

static REGISTER_PROVIDER: Once = Once::new();

pub fn init() {
    REGISTER_PROVIDER.call_once(|| set_key_provider(Arc::new(ClientKeyProvider)));
}

pub fn import_machine_key(source_config: &Path) -> Result<()> {
    let source = source_machine_key_path(source_config)?;
    if !source.exists() {
        return Ok(());
    }
    let data = fs::read(&source).with_context(|| format!("failed to read {}", source.display()))?;
    validate_stored_key(&data, KeyScope::Machine)?;
    atomic_replace(&key_path(KeyScope::Machine), &data)
}

pub fn source_has_machine_key(source_config: &Path) -> bool {
    source_machine_key_path(source_config).map_or(false, |path| path.exists())
}

fn source_machine_key_path(source_config: &Path) -> Result<PathBuf> {
    Ok(source_config
        .parent()
        .ok_or_else(|| anyhow!("source config has no parent directory"))?
        .join(MACHINE_KEY_FILE))
}

struct ClientKeyProvider;

impl KeyProvider for ClientKeyProvider {
    fn key(&self, scope: KeyScope) -> Option<[u8; KEY_LEN]> {
        #[cfg(windows)]
        if scope == KeyScope::User && crate::platform::is_root() {
            hbb_common::throttled_log!(
                Duration::from_secs(30),
                error,
                "The Windows SYSTEM service refused access to the user credential key"
            );
            return None;
        }

        match load_or_create_key(&key_path(scope), scope) {
            Ok(key) => Some(key),
            Err(err) => {
                hbb_common::throttled_log!(
                    Duration::from_secs(30),
                    error,
                    "Failed to load {:?} credential key: {err}",
                    scope
                );
                None
            }
        }
    }
}

fn key_path(scope: KeyScope) -> PathBuf {
    Config::path(match scope {
        KeyScope::Machine => MACHINE_KEY_FILE,
        KeyScope::User => USER_KEY_FILE,
    })
}

fn load_or_create_key(path: &Path, scope: KeyScope) -> Result<[u8; KEY_LEN]> {
    match fs::read(path) {
        Ok(data) => return decode_stored_key(&data, scope),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }

    let mut key = [0u8; KEY_LEN];
    thread_rng().fill(&mut key);
    let stored = encode_stored_key(&key, scope)?;
    if atomic_create(path, &stored)? {
        Ok(key)
    } else {
        decode_stored_key(&fs::read(path)?, scope)
    }
}

fn validate_stored_key(data: &[u8], scope: KeyScope) -> Result<()> {
    decode_stored_key(data, scope).map(|_| ())
}

#[cfg(not(windows))]
fn encode_stored_key(key: &[u8; KEY_LEN], _scope: KeyScope) -> Result<Vec<u8>> {
    Ok(key.to_vec())
}

#[cfg(not(windows))]
fn decode_stored_key(data: &[u8], _scope: KeyScope) -> Result<[u8; KEY_LEN]> {
    data.try_into()
        .map_err(|_| anyhow!("credential key file must contain exactly {KEY_LEN} bytes"))
}

#[cfg(windows)]
fn encode_stored_key(key: &[u8; KEY_LEN], scope: KeyScope) -> Result<Vec<u8>> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{
                CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB,
            },
        },
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: key.len() as u32,
        pbData: key.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let flags = if scope == KeyScope::Machine {
        CRYPTPROTECT_LOCAL_MACHINE
    } else {
        0
    };
    unsafe {
        CryptProtectData(&input, PCWSTR::null(), None, None, None, flags, &mut output)?;
        let protected = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(protected)
    }
}

#[cfg(windows)]
fn decode_stored_key(data: &[u8], _scope: KeyScope) -> Result<[u8; KEY_LEN]> {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB},
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output)?;
        let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let key = plain.try_into().map_err(|_| {
            anyhow!("unprotected credential key must contain exactly {KEY_LEN} bytes")
        });
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        key
    }
}

fn atomic_create(path: &Path, data: &[u8]) -> Result<bool> {
    let temporary = write_temporary(path, data)?;
    let result = match fs::hard_link(&temporary, path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err.into()),
    };
    if let Err(err) = fs::remove_file(&temporary) {
        log::warn!(
            "Failed to remove temporary key file {}: {err}",
            temporary.display()
        );
    }
    result
}

fn atomic_replace(path: &Path, data: &[u8]) -> Result<()> {
    let temporary = write_temporary(path, data)?;
    let result = replace_file(&temporary, path);
    if result.is_err() {
        if let Err(err) = fs::remove_file(&temporary) {
            log::warn!(
                "Failed to remove temporary key file {}: {err}",
                temporary.display()
            );
        }
    }
    result
}

fn write_temporary(path: &Path, data: &[u8]) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("credential key path has no parent directory"))?;
    fs::create_dir_all(parent)?;

    for _ in 0..8 {
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("key"),
            std::process::id(),
            thread_rng().gen::<u64>()
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&temporary) {
            Ok(mut file) => {
                let result = file.write_all(data).and_then(|_| file.sync_all());
                drop(file);
                if let Err(err) = result {
                    if let Err(remove_err) = fs::remove_file(&temporary) {
                        log::warn!(
                            "Failed to remove incomplete key file {}: {remove_err}",
                            temporary.display()
                        );
                    }
                    return Err(err.into());
                }
                return Ok(temporary);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Err(anyhow!(
        "failed to allocate a temporary credential key file"
    ))
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct FileTestProvider(PathBuf);

    impl KeyProvider for FileTestProvider {
        fn key(&self, scope: KeyScope) -> Option<[u8; KEY_LEN]> {
            load_or_create_key(&self.0, scope).ok()
        }
    }

    #[test]
    fn file_key_is_private_and_stable() {
        let directory = std::env::temp_dir().join(format!(
            "clantodesk-keystore-{}-{}",
            std::process::id(),
            thread_rng().gen::<u64>()
        ));
        let path = directory.join(USER_KEY_FILE);

        let first = FileTestProvider(path.clone()).key(KeyScope::User).unwrap();
        let second = FileTestProvider(path.clone()).key(KeyScope::User).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
