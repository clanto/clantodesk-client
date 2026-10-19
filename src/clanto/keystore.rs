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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
type StoredKey = [u8; KEY_LEN];

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendSnapshot {
    Missing,
    Present(StoredKey),
    Unavailable,
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UserKeyChoice {
    Backend(StoredKey, bool),
    ImportFile(StoredKey),
    FileFallback(StoredKey),
    CreateBackend,
    CreateFileFallback,
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackendReadback {
    Missing,
    Present(StoredKey),
    Failed,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UserKeyCacheDecision {
    ReturnCached,
    ResolveSynchronously,
    ResolveInBackground,
}

static REGISTER_PROVIDER: Once = Once::new();

#[cfg(target_os = "linux")]
static USER_KEY_CACHE: std::sync::OnceLock<StoredKey> = std::sync::OnceLock::new();
#[cfg(target_os = "linux")]
static USER_KEY_RESOLUTION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(target_os = "linux")]
static USER_KEY_BACKGROUND_RESOLUTION: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn init() {
    REGISTER_PROVIDER.call_once(|| {
        #[cfg(target_os = "linux")]
        initialize_linux_user_key_cache();
        set_key_provider(Arc::new(ClientKeyProvider));
    });
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
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        if scope == KeyScope::User && crate::platform::is_root() {
            hbb_common::throttled_log!(
                Duration::from_secs(30),
                error,
                "A privileged service refused access to the user credential key"
            );
            return None;
        }

        #[cfg(target_os = "linux")]
        if scope == KeyScope::User {
            return linux_user_key_for_provider();
        }

        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android"
        ))]
        let result = if scope == KeyScope::User {
            load_user_key(&key_path(scope))
        } else {
            load_or_create_key(&key_path(scope), scope)
        };
        #[cfg(not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "android"
        )))]
        let result = load_or_create_key(&key_path(scope), scope);

        match result {
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

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn choose_user_key(
    backend: BackendSnapshot,
    file: Option<StoredKey>,
    fallback_when_unavailable: bool,
) -> Result<UserKeyChoice> {
    match (backend, file) {
        (BackendSnapshot::Present(backend), Some(file)) if backend != file => Err(anyhow!(
            "user credential key differs between the system key store and {USER_KEY_FILE}"
        )),
        (BackendSnapshot::Present(key), file) => Ok(UserKeyChoice::Backend(key, file.is_some())),
        (BackendSnapshot::Missing, Some(key)) => Ok(UserKeyChoice::ImportFile(key)),
        (BackendSnapshot::Missing, None) => Ok(UserKeyChoice::CreateBackend),
        (BackendSnapshot::Unavailable, Some(key)) => Ok(UserKeyChoice::FileFallback(key)),
        (BackendSnapshot::Unavailable, None) if fallback_when_unavailable => {
            Ok(UserKeyChoice::CreateFileFallback)
        }
        (BackendSnapshot::Unavailable, None) => {
            Err(anyhow!("the user credential key store is unavailable"))
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn migration_verified(key: &StoredKey, readback: BackendReadback) -> bool {
    matches!(readback, BackendReadback::Present(value) if value == *key)
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn load_user_key(path: &Path) -> Result<StoredKey> {
    let file_key = read_existing_key_file(path, KeyScope::User)?;
    let backend_result = read_user_key_backend();
    let backend = match &backend_result {
        Ok(Some(key)) => BackendSnapshot::Present(*key),
        Ok(None) => BackendSnapshot::Missing,
        Err(_) => BackendSnapshot::Unavailable,
    };
    let fallback_when_unavailable = user_backend_allows_new_file_when_unavailable(&backend_result);
    let choice = choose_user_key(backend, file_key, fallback_when_unavailable).map_err(|err| {
        let message = backend_result.as_ref().err().map_or_else(
            || err.to_string(),
            |backend_err| format!("{err}: {backend_err}"),
        );
        anyhow!(message)
    })?;

    match choice {
        UserKeyChoice::Backend(key, remove_file) => {
            if remove_file {
                let readback = read_backend_for_verification();
                if migration_verified(&key, readback) {
                    remove_migrated_key_file(path);
                }
            }
            Ok(key)
        }
        UserKeyChoice::ImportFile(key) => persist_user_backend(path, key, true),
        UserKeyChoice::FileFallback(key) => {
            if let Err(err) = backend_result {
                log_user_backend_fallback(&err);
            }
            Ok(key)
        }
        UserKeyChoice::CreateBackend => {
            let mut key = [0u8; KEY_LEN];
            thread_rng().fill(&mut key);
            persist_user_backend(path, key, false)
        }
        UserKeyChoice::CreateFileFallback => {
            if let Err(err) = backend_result {
                log_user_backend_fallback(&err);
            }
            load_or_create_key(path, KeyScope::User)
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn persist_user_backend(path: &Path, key: StoredKey, migrating_file: bool) -> Result<StoredKey> {
    let write_error = write_user_key_backend(&key).err();
    let readback_result = read_user_key_backend();
    let readback = match &readback_result {
        Ok(Some(value)) => BackendReadback::Present(*value),
        Ok(None) => BackendReadback::Missing,
        Err(_) => BackendReadback::Failed,
    };
    if migration_verified(&key, readback) {
        if migrating_file {
            remove_migrated_key_file(path);
        }
        return Ok(key);
    }
    if let BackendReadback::Present(_) = readback {
        return Err(anyhow!(
            "the user credential key store changed while the key was being saved"
        ));
    }
    let file_fallback_allowed = migrating_file
        || user_backend_has_file_fallback(write_error.as_ref(), readback_result.as_ref().err());

    if let Some(err) = write_error.as_ref() {
        log_user_backend_failure(err, file_fallback_allowed);
    } else if let Err(err) = readback_result.as_ref() {
        log_user_backend_failure(err, file_fallback_allowed);
    } else {
        hbb_common::throttled_log!(
            Duration::from_secs(30),
            warn,
            "The user credential key store did not return the key after saving it"
        );
    }

    if migrating_file {
        return Ok(key);
    }
    if file_fallback_allowed {
        if atomic_create(path, &encode_stored_key(&key, KeyScope::User)?)? {
            Ok(key)
        } else {
            decode_stored_key(&fs::read(path)?, KeyScope::User)
        }
    } else {
        Err(write_error
            .or_else(|| readback_result.err())
            .unwrap_or_else(|| anyhow!("failed to verify the saved user credential key")))
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn read_existing_key_file(path: &Path, scope: KeyScope) -> Result<Option<StoredKey>> {
    match fs::read(path) {
        Ok(data) => decode_stored_key(&data, scope).map(Some),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err.into()),
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn read_backend_for_verification() -> BackendReadback {
    match read_user_key_backend() {
        Ok(Some(key)) => BackendReadback::Present(key),
        Ok(None) => BackendReadback::Missing,
        Err(_) => BackendReadback::Failed,
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn remove_migrated_key_file(path: &Path) {
    if let Err(err) = fs::remove_file(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            log::warn!(
                "Failed to remove migrated credential key file {}: {err}",
                path.display()
            );
        }
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn log_user_backend_fallback(err: &hbb_common::anyhow::Error) {
    hbb_common::throttled_log!(
        Duration::from_secs(30),
        warn,
        "The user credential key store is unavailable; using the protected file fallback: {err}"
    );
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "linux",
    target_os = "android"
))]
fn log_user_backend_failure(err: &hbb_common::anyhow::Error, file_fallback_allowed: bool) {
    if file_fallback_allowed {
        log_user_backend_fallback(err);
    } else {
        hbb_common::throttled_log!(
            Duration::from_secs(30),
            warn,
            "The user credential key store is unavailable; no new file fallback was created: {err}"
        );
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
const APPLE_KEYCHAIN_SERVICE: &str = "it.clanto.ClantoDesk";
#[cfg(any(target_os = "macos", target_os = "ios"))]
const USER_KEY_ACCOUNT: &str = "user-key";

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn user_backend_has_file_fallback(
    _write_error: Option<&hbb_common::anyhow::Error>,
    _readback_error: Option<&hbb_common::anyhow::Error>,
) -> bool {
    false
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn user_backend_allows_new_file_when_unavailable(
    _backend_result: &Result<Option<StoredKey>>,
) -> bool {
    false
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn read_user_key_backend() -> Result<Option<StoredKey>> {
    use security_framework::passwords::{generic_password, PasswordOptions};

    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
    let mut options =
        PasswordOptions::new_generic_password(APPLE_KEYCHAIN_SERVICE, USER_KEY_ACCOUNT);
    options.set_access_synchronized(Some(false));
    match generic_password(options) {
        Ok(data) => decode_stored_key(&data, KeyScope::User).map(Some),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(err) => Err(anyhow!("failed to read the user key from Keychain: {err}")),
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn write_user_key_backend(key: &StoredKey) -> Result<()> {
    use security_framework::{
        access_control::{ProtectionMode, SecAccessControl},
        passwords::{set_generic_password_options, AccessControlOptions, PasswordOptions},
    };

    let access_control = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
        AccessControlOptions::empty().bits(),
    )
    .map_err(|err| anyhow!("failed to configure Keychain accessibility: {err}"))?;
    let mut options =
        PasswordOptions::new_generic_password(APPLE_KEYCHAIN_SERVICE, USER_KEY_ACCOUNT);
    options.set_access_control(access_control);
    options.set_access_synchronized(Some(false));
    set_generic_password_options(key, options)
        .map_err(|err| anyhow!("failed to save the user key in Keychain: {err}"))
}

#[cfg(target_os = "linux")]
fn user_backend_has_file_fallback(
    write_error: Option<&hbb_common::anyhow::Error>,
    readback_error: Option<&hbb_common::anyhow::Error>,
) -> bool {
    write_error.into_iter().chain(readback_error).all(|err| {
        err.downcast_ref::<LinuxUserKeyStoreMustNotFallback>()
            .is_none()
    })
}

#[cfg(target_os = "android")]
fn user_backend_has_file_fallback(
    _write_error: Option<&hbb_common::anyhow::Error>,
    _readback_error: Option<&hbb_common::anyhow::Error>,
) -> bool {
    true
}

#[cfg(target_os = "linux")]
fn user_backend_allows_new_file_when_unavailable(
    backend_result: &Result<Option<StoredKey>>,
) -> bool {
    backend_result.as_ref().err().map_or(true, |err| {
        err.downcast_ref::<LinuxUserKeyStoreMustNotFallback>()
            .is_none()
    })
}

#[cfg(target_os = "android")]
fn user_backend_allows_new_file_when_unavailable(
    _backend_result: &Result<Option<StoredKey>>,
) -> bool {
    android_has_stored_user_key().map_or(false, |stored| !stored)
}

#[cfg(target_os = "android")]
fn android_has_stored_user_key() -> Result<bool> {
    use jni::objects::JValue;

    scrap::android::with_application_context(|jvm, context| {
        let mut env = jvm
            .attach_current_thread()
            .map_err(|err| anyhow!("failed to attach to the Android JVM: {err}"))?;
        let class = android_user_key_store_class(&mut env, context.as_obj())?;
        env.call_static_method(
            class,
            "hasStoredUserKey",
            "(Landroid/content/Context;)Z",
            &[JValue::Object(context.as_obj())],
        )
        .map_err(|err| match env.exception_clear() {
            Ok(()) => anyhow!("failed to inspect the Android Keystore user key: {err}"),
            Err(clear_err) => anyhow!(
                "failed to inspect the Android Keystore user key: {err}; failed to clear the Java exception: {clear_err}"
            ),
        })?
        .z()
        .map_err(|err| anyhow!("invalid Android Keystore result: {err}"))
    })
    .ok_or_else(|| anyhow!("Android application context is not initialized"))?
}

#[cfg(target_os = "android")]
fn read_user_key_backend() -> Result<Option<StoredKey>> {
    use jni::objects::{JByteArray, JValue};

    scrap::android::with_application_context(|jvm, context| {
        let mut env = jvm
            .attach_current_thread()
            .map_err(|err| anyhow!("failed to attach to the Android JVM: {err}"))?;
        let class = android_user_key_store_class(&mut env, context.as_obj())?;
        let value = env
            .call_static_method(
                class,
                "readUserKey",
                "(Landroid/content/Context;)[B",
                &[JValue::Object(context.as_obj())],
            )
            .map_err(|err| match env.exception_clear() {
                Ok(()) => anyhow!("failed to read the Android Keystore user key: {err}"),
                Err(clear_err) => anyhow!(
                    "failed to read the Android Keystore user key: {err}; failed to clear the Java exception: {clear_err}"
                ),
            })?;
        let object = value
            .l()
            .map_err(|err| anyhow!("invalid Android Keystore result: {err}"))?;
        if object.is_null() {
            return Ok(None);
        }
        let array = JByteArray::from(object);
        let data = env
            .convert_byte_array(&array)
            .map_err(|err| anyhow!("failed to copy the Android Keystore user key: {err}"))?;
        decode_stored_key(&data, KeyScope::User).map(Some)
    })
    .ok_or_else(|| anyhow!("Android application context is not initialized"))?
}

#[cfg(target_os = "android")]
fn write_user_key_backend(key: &StoredKey) -> Result<()> {
    use jni::objects::{JObject, JValue};

    scrap::android::with_application_context(|jvm, context| {
        let mut env = jvm
            .attach_current_thread()
            .map_err(|err| anyhow!("failed to attach to the Android JVM: {err}"))?;
        let class = android_user_key_store_class(&mut env, context.as_obj())?;
        let array = env
            .byte_array_from_slice(key)
            .map_err(|err| anyhow!("failed to copy the Android user key: {err}"))?;
        let array = JObject::from(array);
        let stored = env
            .call_static_method(
                class,
                "storeUserKey",
                "(Landroid/content/Context;[B)Z",
                &[JValue::Object(context.as_obj()), JValue::Object(&array)],
            )
            .map_err(|err| match env.exception_clear() {
                Ok(()) => anyhow!("failed to save the Android Keystore user key: {err}"),
                Err(clear_err) => anyhow!(
                    "failed to save the Android Keystore user key: {err}; failed to clear the Java exception: {clear_err}"
                ),
            })?
            .z()
            .map_err(|err| anyhow!("invalid Android Keystore result: {err}"))?;
        if stored {
            Ok(())
        } else {
            Err(anyhow!("Android Keystore did not store the user key"))
        }
    })
    .ok_or_else(|| anyhow!("Android application context is not initialized"))?
}

#[cfg(target_os = "android")]
fn android_user_key_store_class<'local>(
    env: &mut jni::JNIEnv<'local>,
    context: &jni::objects::JObject,
) -> Result<jni::objects::JClass<'local>> {
    use jni::objects::{JClass, JObject, JValue};

    let loader = env
        .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
        .and_then(|value| value.l())
        .map_err(|err| android_jni_error(env, "get the Android application class loader", err))?;
    let name = JObject::from(
        env.new_string("it.clanto.clantodesk.clanto.ClantoUserKeyStore")
            .map_err(|err| android_jni_error(env, "create the Android class name", err))?,
    );
    let class = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )
        .and_then(|value| value.l())
        .map_err(|err| android_jni_error(env, "load the Android user key store class", err))?;
    Ok(JClass::from(class))
}

#[cfg(target_os = "android")]
fn android_jni_error(
    env: &mut jni::JNIEnv,
    action: &str,
    err: jni::errors::Error,
) -> hbb_common::anyhow::Error {
    match env.exception_clear() {
        Ok(()) => anyhow!("failed to {action}: {err}"),
        Err(clear_err) => {
            anyhow!("failed to {action}: {err}; failed to clear the Java exception: {clear_err}")
        }
    }
}

#[cfg(target_os = "linux")]
const SECRET_SERVICE_DESTINATION: &str = "org.freedesktop.secrets";
#[cfg(target_os = "linux")]
const SECRET_SERVICE_PATH: &str = "/org/freedesktop/secrets";
#[cfg(target_os = "linux")]
const SECRET_SERVICE_INTERFACE: &str = "org.freedesktop.Secret.Service";
#[cfg(target_os = "linux")]
const SECRET_COLLECTION_INTERFACE: &str = "org.freedesktop.Secret.Collection";
#[cfg(target_os = "linux")]
const SECRET_ITEM_INTERFACE: &str = "org.freedesktop.Secret.Item";
#[cfg(target_os = "linux")]
const SECRET_SESSION_INTERFACE: &str = "org.freedesktop.Secret.Session";
#[cfg(target_os = "linux")]
const SECRET_PROMPT_INTERFACE: &str = "org.freedesktop.Secret.Prompt";
#[cfg(target_os = "linux")]
const SECRET_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "linux")]
const SECRET_PROMPT_TIMEOUT: Duration = Duration::from_secs(300);

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxUserKeyStoreMustNotFallback(String);

#[cfg(target_os = "linux")]
impl std::fmt::Display for LinuxUserKeyStoreMustNotFallback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "Secret Service may already contain the user key and is unavailable: {}",
            self.0
        )
    }
}

#[cfg(target_os = "linux")]
impl std::error::Error for LinuxUserKeyStoreMustNotFallback {}

#[cfg(target_os = "linux")]
fn user_key_cache_decision(cached: bool, inside_tokio_runtime: bool) -> UserKeyCacheDecision {
    if cached {
        UserKeyCacheDecision::ReturnCached
    } else if inside_tokio_runtime {
        UserKeyCacheDecision::ResolveInBackground
    } else {
        UserKeyCacheDecision::ResolveSynchronously
    }
}

#[cfg(target_os = "linux")]
fn cache_user_key(cache: &std::sync::OnceLock<StoredKey>, key: StoredKey) -> StoredKey {
    *cache.get_or_init(|| key)
}

#[cfg(target_os = "linux")]
fn initialize_linux_user_key_cache() {
    if crate::platform::is_root() {
        return;
    }
    if let Err(err) = resolve_linux_user_key() {
        log_linux_user_key_resolution_error(&err);
    }
}

#[cfg(target_os = "linux")]
fn linux_user_key_for_provider() -> Option<StoredKey> {
    let cached = USER_KEY_CACHE.get().copied();
    let inside_tokio_runtime = hbb_common::tokio::runtime::Handle::try_current().is_ok();
    match user_key_cache_decision(cached.is_some(), inside_tokio_runtime) {
        UserKeyCacheDecision::ReturnCached => cached,
        UserKeyCacheDecision::ResolveSynchronously => match resolve_linux_user_key() {
            Ok(key) => Some(key),
            Err(err) => {
                log_linux_user_key_resolution_error(&err);
                None
            }
        },
        UserKeyCacheDecision::ResolveInBackground => {
            start_linux_user_key_resolution();
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn resolve_linux_user_key() -> Result<StoredKey> {
    if let Some(key) = USER_KEY_CACHE.get() {
        return Ok(*key);
    }
    let _resolution = USER_KEY_RESOLUTION_LOCK.lock().unwrap();
    if let Some(key) = USER_KEY_CACHE.get() {
        return Ok(*key);
    }
    let key = load_user_key(&key_path(KeyScope::User))?;
    Ok(cache_user_key(&USER_KEY_CACHE, key))
}

#[cfg(target_os = "linux")]
fn start_linux_user_key_resolution() {
    use std::sync::atomic::Ordering;

    if USER_KEY_BACKGROUND_RESOLUTION
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if let Err(err) = std::thread::Builder::new()
        .name("clanto-user-key".to_owned())
        .spawn(|| {
            let result = resolve_linux_user_key();
            USER_KEY_BACKGROUND_RESOLUTION.store(false, Ordering::Release);
            if let Err(err) = result {
                log_linux_user_key_resolution_error(&err);
            }
        })
    {
        USER_KEY_BACKGROUND_RESOLUTION.store(false, Ordering::Release);
        let err: hbb_common::anyhow::Error = err.into();
        log_linux_user_key_resolution_error(&err);
    }
}

#[cfg(target_os = "linux")]
fn log_linux_user_key_resolution_error(err: &hbb_common::anyhow::Error) {
    hbb_common::throttled_log!(
        Duration::from_secs(30),
        error,
        "Failed to resolve the Linux user credential key: {err}"
    );
}

#[cfg(target_os = "linux")]
fn read_user_key_backend() -> Result<Option<StoredKey>> {
    linux_secret_service_read()
}

#[cfg(target_os = "linux")]
fn write_user_key_backend(key: &StoredKey) -> Result<()> {
    linux_secret_service_write(key)
}

#[cfg(target_os = "linux")]
fn linux_secret_service_read() -> Result<Option<StoredKey>> {
    use dbus::blocking::Connection;

    let connection = Connection::new_session().context("failed to connect to the session bus")?;
    let Some(collection) = linux_default_collection(&connection, false)? else {
        return Ok(None);
    };
    linux_secret_service_read_collection(&connection, collection)
        .map_err(|err| anyhow!(LinuxUserKeyStoreMustNotFallback(err.to_string())))
}

#[cfg(target_os = "linux")]
fn linux_secret_service_read_collection(
    connection: &dbus::blocking::Connection,
    collection: dbus::Path<'static>,
) -> Result<Option<StoredKey>> {
    use dbus::{blocking::stdintf::org_freedesktop_dbus::Properties, Path};

    linux_unlock_paths(
        connection,
        std::slice::from_ref(&collection),
        SECRET_COLLECTION_INTERFACE,
    )?;
    let collection_proxy = connection.with_proxy(
        SECRET_SERVICE_DESTINATION,
        collection.clone(),
        SECRET_TIMEOUT,
    );
    let (items,): (Vec<Path<'static>>,) = collection_proxy.method_call(
        SECRET_COLLECTION_INTERFACE,
        "SearchItems",
        (linux_secret_attributes(),),
    )?;
    if items.is_empty() {
        return Ok(None);
    }

    let mut locked = Vec::new();
    for item in &items {
        let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, item.clone(), SECRET_TIMEOUT);
        if proxy.get::<bool>(SECRET_ITEM_INTERFACE, "Locked")? {
            locked.push(item.clone());
        }
    }
    linux_unlock_paths(connection, &locked, SECRET_ITEM_INTERFACE)?;

    let session = linux_open_secret_session(connection)?;
    let result = (|| {
        let mut found = None;
        for item in items {
            let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, item, SECRET_TIMEOUT);
            let (secret,): ((Path<'static>, Vec<u8>, Vec<u8>, String),) =
                proxy.method_call(SECRET_ITEM_INTERFACE, "GetSecret", (session.clone(),))?;
            let key = decode_stored_key(&secret.2, KeyScope::User)?;
            if found.is_some_and(|value| value != key) {
                return Err(anyhow!(
                    "Secret Service contains conflicting ClantoDesk user keys"
                ));
            }
            found = Some(key);
        }
        Ok(found)
    })();
    linux_close_secret_session(connection, session);
    result
}

#[cfg(target_os = "linux")]
fn linux_secret_service_write(key: &StoredKey) -> Result<()> {
    let connection = dbus::blocking::Connection::new_session()
        .context("failed to connect to the session bus")?;
    let collection = linux_default_collection(&connection, true)?
        .ok_or_else(|| anyhow!("Secret Service has no default collection"))?;
    linux_secret_service_write_collection(&connection, collection, key)
        .map_err(|err| anyhow!(LinuxUserKeyStoreMustNotFallback(err.to_string())))
}

#[cfg(target_os = "linux")]
fn linux_secret_service_write_collection(
    connection: &dbus::blocking::Connection,
    collection: dbus::Path<'static>,
    key: &StoredKey,
) -> Result<()> {
    use dbus::arg::{PropMap, RefArg, Variant};

    linux_unlock_paths(
        connection,
        std::slice::from_ref(&collection),
        SECRET_COLLECTION_INTERFACE,
    )?;
    let session = linux_open_secret_session(connection)?;
    let result = (|| {
        let mut properties = PropMap::new();
        properties.insert(
            "org.freedesktop.Secret.Item.Label".to_owned(),
            Variant(Box::new("ClantoDesk user key".to_owned()) as Box<dyn RefArg>),
        );
        properties.insert(
            "org.freedesktop.Secret.Item.Attributes".to_owned(),
            Variant(Box::new(linux_secret_attributes()) as Box<dyn RefArg>),
        );
        let secret = (
            session.clone(),
            Vec::<u8>::new(),
            key.to_vec(),
            "application/octet-stream".to_owned(),
        );
        let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, collection, SECRET_TIMEOUT);
        let (item, prompt): (dbus::Path<'static>, dbus::Path<'static>) = proxy.method_call(
            SECRET_COLLECTION_INTERFACE,
            "CreateItem",
            (properties, secret, false),
        )?;
        if &*item == "/" {
            let created = linux_wait_path_prompt(connection, prompt)?;
            if &*created == "/" {
                return Err(anyhow!("Secret Service did not create the user key item"));
            }
        }
        Ok(())
    })();
    linux_close_secret_session(connection, session);
    result
}

#[cfg(target_os = "linux")]
fn linux_secret_attributes() -> std::collections::HashMap<String, String> {
    [
        ("application".to_owned(), "it.clanto.ClantoDesk".to_owned()),
        ("account".to_owned(), "user-key".to_owned()),
    ]
    .into_iter()
    .collect()
}

#[cfg(target_os = "linux")]
fn linux_default_collection(
    connection: &dbus::blocking::Connection,
    create: bool,
) -> Result<Option<dbus::Path<'static>>> {
    use dbus::arg::{PropMap, RefArg, Variant};

    let service = connection.with_proxy(
        SECRET_SERVICE_DESTINATION,
        SECRET_SERVICE_PATH,
        SECRET_TIMEOUT,
    );
    let collection_result: std::result::Result<(dbus::Path<'static>,), dbus::Error> =
        service.method_call(SECRET_SERVICE_INTERFACE, "ReadAlias", ("default",));
    let (collection,) = match collection_result {
        Ok(collection) => collection,
        Err(err) if linux_secret_service_is_absent(&err) => return Err(err.into()),
        Err(err) => return Err(anyhow!(LinuxUserKeyStoreMustNotFallback(err.to_string()))),
    };
    if &*collection != "/" {
        return Ok(Some(collection));
    }
    if !create {
        return Ok(None);
    }

    let mut properties = PropMap::new();
    properties.insert(
        "org.freedesktop.Secret.Collection.Label".to_owned(),
        Variant(Box::new("Default".to_owned()) as Box<dyn RefArg>),
    );
    let (collection, prompt): (dbus::Path<'static>, dbus::Path<'static>) = service.method_call(
        SECRET_SERVICE_INTERFACE,
        "CreateCollection",
        (properties, "default"),
    )?;
    if &*collection != "/" {
        Ok(Some(collection))
    } else {
        let collection = linux_wait_path_prompt(connection, prompt)?;
        if &*collection == "/" {
            Err(anyhow!(
                "Secret Service did not create the default collection"
            ))
        } else {
            Ok(Some(collection))
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_secret_service_is_absent(err: &dbus::Error) -> bool {
    matches!(
        err.name(),
        Some("org.freedesktop.DBus.Error.ServiceUnknown")
            | Some("org.freedesktop.DBus.Error.NameHasNoOwner")
            | Some("org.freedesktop.DBus.Error.Spawn.ServiceNotFound")
    )
}

#[cfg(target_os = "linux")]
fn linux_unlock_paths(
    connection: &dbus::blocking::Connection,
    paths: &[dbus::Path<'static>],
    interface: &str,
) -> Result<()> {
    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;

    if paths.is_empty() {
        return Ok(());
    }
    let mut locked = Vec::new();
    for path in paths {
        let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, path.clone(), SECRET_TIMEOUT);
        if proxy.get::<bool>(interface, "Locked")? {
            locked.push(path.clone());
        }
    }
    if locked.is_empty() {
        return Ok(());
    }

    let service = connection.with_proxy(
        SECRET_SERVICE_DESTINATION,
        SECRET_SERVICE_PATH,
        SECRET_TIMEOUT,
    );
    let (unlocked, prompt): (Vec<dbus::Path<'static>>, dbus::Path<'static>) =
        service.method_call(SECRET_SERVICE_INTERFACE, "Unlock", (locked.clone(),))?;
    if locked.iter().all(|path| unlocked.contains(path)) {
        return Ok(());
    }
    if &*prompt == "/" {
        return Err(anyhow!(
            "Secret Service did not unlock the requested objects"
        ));
    }
    linux_wait_paths_prompt(connection, prompt)?;
    for path in locked {
        let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, path, SECRET_TIMEOUT);
        if proxy.get::<bool>(interface, "Locked")? {
            return Err(anyhow!(
                "Secret Service object remains locked after the prompt"
            ));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_open_secret_session(
    connection: &dbus::blocking::Connection,
) -> Result<dbus::Path<'static>> {
    use dbus::arg::Variant;

    let service = connection.with_proxy(
        SECRET_SERVICE_DESTINATION,
        SECRET_SERVICE_PATH,
        SECRET_TIMEOUT,
    );
    let (_, session): (Variant<String>, dbus::Path<'static>) = service.method_call(
        SECRET_SERVICE_INTERFACE,
        "OpenSession",
        // The plain algorithm is confined to the current user's session bus.
        ("plain", Variant(String::new())),
    )?;
    Ok(session)
}

#[cfg(target_os = "linux")]
fn linux_close_secret_session(
    connection: &dbus::blocking::Connection,
    session: dbus::Path<'static>,
) {
    let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, session, SECRET_TIMEOUT);
    let result: std::result::Result<(), dbus::Error> =
        proxy.method_call(SECRET_SESSION_INTERFACE, "Close", ());
    if let Err(err) = result {
        log::trace!("Failed to close the Secret Service session: {err}");
    }
}

#[cfg(target_os = "linux")]
fn linux_wait_path_prompt(
    connection: &dbus::blocking::Connection,
    prompt: dbus::Path<'static>,
) -> Result<dbus::Path<'static>> {
    use dbus::{arg::Variant, message::MatchRule, Path};

    if &*prompt == "/" {
        return Err(anyhow!("Secret Service returned no prompt result"));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let rule = MatchRule::new_signal(SECRET_PROMPT_INTERFACE, "Completed")
        .with_sender(SECRET_SERVICE_DESTINATION)
        .with_path(prompt.clone());
    let token = connection.add_match(
        rule,
        move |(dismissed, Variant(path)): (bool, Variant<Path<'static>>), _, _| {
            let _ = sender.send((dismissed, path));
            true
        },
    )?;
    let result = linux_start_prompt(connection, prompt.clone())
        .and_then(|_| linux_process_path_prompt(connection, prompt, receiver));
    if let Err(err) = connection.remove_match(token) {
        log::trace!("Failed to remove the Secret Service prompt match: {err}");
    }
    result
}

#[cfg(target_os = "linux")]
fn linux_wait_paths_prompt(
    connection: &dbus::blocking::Connection,
    prompt: dbus::Path<'static>,
) -> Result<Vec<dbus::Path<'static>>> {
    use dbus::{arg::Variant, message::MatchRule, Path};

    if &*prompt == "/" {
        return Err(anyhow!("Secret Service returned no prompt result"));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let rule = MatchRule::new_signal(SECRET_PROMPT_INTERFACE, "Completed")
        .with_sender(SECRET_SERVICE_DESTINATION)
        .with_path(prompt.clone());
    let token = connection.add_match(
        rule,
        move |(dismissed, Variant(paths)): (bool, Variant<Vec<Path<'static>>>), _, _| {
            let _ = sender.send((dismissed, paths));
            true
        },
    )?;
    let result = linux_start_prompt(connection, prompt.clone())
        .and_then(|_| linux_process_paths_prompt(connection, prompt, receiver));
    if let Err(err) = connection.remove_match(token) {
        log::trace!("Failed to remove the Secret Service prompt match: {err}");
    }
    result
}

#[cfg(target_os = "linux")]
fn linux_start_prompt(
    connection: &dbus::blocking::Connection,
    prompt: dbus::Path<'static>,
) -> Result<()> {
    let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, prompt, SECRET_TIMEOUT);
    let _: () = proxy.method_call(SECRET_PROMPT_INTERFACE, "Prompt", ("",))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_process_path_prompt(
    connection: &dbus::blocking::Connection,
    prompt: dbus::Path<'static>,
    receiver: std::sync::mpsc::Receiver<(bool, dbus::Path<'static>)>,
) -> Result<dbus::Path<'static>> {
    let deadline = std::time::Instant::now() + SECRET_PROMPT_TIMEOUT;
    loop {
        match receiver.try_recv() {
            Ok((false, path)) => return Ok(path),
            Ok((true, _)) => return Err(anyhow!("Secret Service prompt was dismissed")),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(anyhow!("Secret Service prompt channel disconnected"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        if std::time::Instant::now() >= deadline {
            linux_dismiss_prompt(connection, prompt);
            return Err(anyhow!("Secret Service prompt timed out"));
        }
        connection.process(Duration::from_secs(1))?;
    }
}

#[cfg(target_os = "linux")]
fn linux_process_paths_prompt(
    connection: &dbus::blocking::Connection,
    prompt: dbus::Path<'static>,
    receiver: std::sync::mpsc::Receiver<(bool, Vec<dbus::Path<'static>>)>,
) -> Result<Vec<dbus::Path<'static>>> {
    let deadline = std::time::Instant::now() + SECRET_PROMPT_TIMEOUT;
    loop {
        match receiver.try_recv() {
            Ok((false, paths)) => return Ok(paths),
            Ok((true, _)) => return Err(anyhow!("Secret Service prompt was dismissed")),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(anyhow!("Secret Service prompt channel disconnected"));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        if std::time::Instant::now() >= deadline {
            linux_dismiss_prompt(connection, prompt);
            return Err(anyhow!("Secret Service prompt timed out"));
        }
        connection.process(Duration::from_secs(1))?;
    }
}

#[cfg(target_os = "linux")]
fn linux_dismiss_prompt(connection: &dbus::blocking::Connection, prompt: dbus::Path<'static>) {
    let proxy = connection.with_proxy(SECRET_SERVICE_DESTINATION, prompt, SECRET_TIMEOUT);
    let result: std::result::Result<(), dbus::Error> =
        proxy.method_call(SECRET_PROMPT_INTERFACE, "Dismiss", ());
    if let Err(err) = result {
        log::trace!("Failed to dismiss the Secret Service prompt: {err}");
    }
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

    #[test]
    fn user_key_choice_prefers_an_existing_backend_key() {
        let key = [1; KEY_LEN];

        assert_eq!(
            choose_user_key(BackendSnapshot::Present(key), None, true).unwrap(),
            UserKeyChoice::Backend(key, false)
        );
        assert_eq!(
            choose_user_key(BackendSnapshot::Present(key), Some(key), true).unwrap(),
            UserKeyChoice::Backend(key, true)
        );
    }

    #[test]
    fn user_key_choice_imports_or_falls_back_to_an_existing_file() {
        let key = [2; KEY_LEN];

        assert_eq!(
            choose_user_key(BackendSnapshot::Missing, Some(key), true).unwrap(),
            UserKeyChoice::ImportFile(key)
        );
        assert_eq!(
            choose_user_key(BackendSnapshot::Unavailable, Some(key), true).unwrap(),
            UserKeyChoice::FileFallback(key)
        );
    }

    #[test]
    fn user_key_choice_creates_only_when_no_key_exists() {
        assert_eq!(
            choose_user_key(BackendSnapshot::Missing, None, true).unwrap(),
            UserKeyChoice::CreateBackend
        );
        assert_eq!(
            choose_user_key(BackendSnapshot::Unavailable, None, true).unwrap(),
            UserKeyChoice::CreateFileFallback
        );
        assert!(choose_user_key(BackendSnapshot::Unavailable, None, false).is_err());
    }

    #[test]
    fn user_key_choice_rejects_conflicting_existing_keys() {
        assert!(choose_user_key(
            BackendSnapshot::Present([3; KEY_LEN]),
            Some([4; KEY_LEN]),
            true,
        )
        .is_err());
    }

    #[test]
    fn migration_requires_an_identical_backend_readback() {
        let key = [5; KEY_LEN];

        assert!(migration_verified(&key, BackendReadback::Present(key)));
        assert!(!migration_verified(&key, BackendReadback::Missing));
        assert!(!migration_verified(&key, BackendReadback::Failed));
        assert!(!migration_verified(
            &key,
            BackendReadback::Present([6; KEY_LEN])
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn user_key_cache_decision_never_resolves_on_a_tokio_worker() {
        assert_eq!(
            user_key_cache_decision(false, true),
            UserKeyCacheDecision::ResolveInBackground
        );
        assert_eq!(
            user_key_cache_decision(false, false),
            UserKeyCacheDecision::ResolveSynchronously
        );
        assert_eq!(
            user_key_cache_decision(true, true),
            UserKeyCacheDecision::ReturnCached
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn user_key_cache_keeps_the_first_resolved_key() {
        let cache = std::sync::OnceLock::new();

        assert_eq!(cache_user_key(&cache, [7; KEY_LEN]), [7; KEY_LEN]);
        assert_eq!(cache_user_key(&cache, [8; KEY_LEN]), [7; KEY_LEN]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn existing_locked_collection_disables_new_file_fallback() {
        let backend_result = Err(anyhow!(LinuxUserKeyStoreMustNotFallback(
            "prompt dismissed".to_owned()
        )));
        let fallback_allowed = user_backend_allows_new_file_when_unavailable(&backend_result);
        assert!(!fallback_allowed);
        assert!(choose_user_key(BackendSnapshot::Unavailable, None, fallback_allowed).is_err());
        assert!(!user_backend_has_file_fallback(
            backend_result.as_ref().err(),
            None
        ));
    }
}
