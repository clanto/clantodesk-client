// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

use hbb_common::{
    anyhow::{anyhow, Context, Result},
    config::{load_path, store_path, Config, Config2},
    get_exe_time, get_modified_time, log, toml,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn should_import_config(
    destination_has_identity: bool,
    source_newer: bool,
    source_older_than_exe: bool,
) -> bool {
    !destination_has_identity && source_newer && source_older_than_exe
}

pub fn import_config(source_config: &Path) {
    let source_config2 = PathBuf::from(source_config.to_string_lossy().replace(".toml", "2.toml"));
    let destination_config = Config::file();
    let destination_config2 = Config2::file();
    let source_newer = get_modified_time(source_config) > get_modified_time(&destination_config);
    let source_older_than_exe = get_modified_time(source_config) < get_exe_time();
    let source_config2_newer =
        get_modified_time(&source_config2) > get_modified_time(&destination_config2);

    if let Err(err) = import_config_files(
        source_config,
        &source_config2,
        &destination_config,
        &destination_config2,
        source_newer,
        source_older_than_exe,
        source_config2_newer,
    ) {
        log::error!("Failed to import configuration: {err}");
    }
}

fn import_config_files(
    source_config: &Path,
    source_config2: &Path,
    destination_config: &Path,
    destination_config2: &Path,
    source_newer: bool,
    source_older_than_exe: bool,
    source_config2_newer: bool,
) -> Result<()> {
    log::info!(
        "import config from {:?} and {:?}",
        source_config,
        source_config2
    );

    let destination_has_identity = destination_has_identity(destination_config)?;
    if destination_has_identity {
        log::info!(
            "Service configuration already has an identity; keeping its Config, Config2, and machine key"
        );
        return Ok(());
    }

    let config: Config = load_path(source_config.to_path_buf());
    if config.is_empty() {
        log::info!("Empty source config, skipped");
        return Ok(());
    }

    let import_config = should_import_config(
        destination_has_identity,
        source_newer,
        source_older_than_exe,
    );
    let import_config2 = source_config2_newer;
    let source_has_machine_key = crate::clanto::keystore::source_has_machine_key(source_config);

    if import_config {
        backup_if_replaced(destination_config)?;
        if import_config2 || (source_has_machine_key && destination_config2.exists()) {
            backup_if_replaced(destination_config2)?;
        }
        crate::clanto::keystore::import_machine_key_to(
            source_config,
            destination_config,
            destination_config2,
            import_config2,
        )?;
    } else if import_config2 && source_has_machine_key {
        log::error!(
            "Refusing to import Config2 without Config because its protected machine key may differ"
        );
        return Ok(());
    }

    if import_config {
        match store_path(destination_config.to_path_buf(), config) {
            Ok(()) => log::info!("config written"),
            Err(err) => {
                log::error!("Failed to write config: {err}");
                return Err(err.into());
            }
        }
    }
    if import_config2 {
        if !import_config {
            backup_if_replaced(destination_config2)?;
        }
        let config2: Config2 = load_path(source_config2.to_path_buf());
        match store_path(destination_config2.to_path_buf(), config2) {
            Ok(()) => log::info!("config2 written"),
            Err(err) => {
                log::error!("Failed to write config2: {err}");
                return Err(err.into());
            }
        }
    }
    Ok(())
}

fn destination_has_identity(path: &Path) -> Result<bool> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let config: toml::Value =
        toml::from_str(&source).with_context(|| format!("failed to parse {}", path.display()))?;
    let table = config
        .as_table()
        .ok_or_else(|| anyhow!("{} is not a TOML table", path.display()))?;
    Ok(["id", "enc_id", "key_pair"]
        .iter()
        .filter_map(|key| table.get(*key))
        .any(value_has_data))
}

fn value_has_data(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(value) => !value.is_empty(),
        toml::Value::Array(values) => values.iter().any(value_has_data),
        toml::Value::Table(values) => values.values().any(value_has_data),
        _ => true,
    }
}

fn backup_if_replaced(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut backup = path.as_os_str().to_owned();
    backup.push(".pre-import");
    let backup = PathBuf::from(backup);
    fs::copy(path, &backup).with_context(|| {
        format!(
            "failed to back up {} to {}",
            path.display(),
            backup.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use hbb_common::password_security::{
        decrypt_str_or_original_scoped, set_key_provider, KeyProvider, KeyScope,
    };
    use hbb_common::{
        password_security::{
            decrypt_str_or_original, encrypt_str_or_original, encrypt_vec_or_original,
        },
        rand::{thread_rng, Rng},
    };
    use serde::Serialize;
    use std::collections::HashMap;
    #[cfg(target_os = "linux")]
    use std::sync::Arc;

    #[cfg(target_os = "linux")]
    const SERVICE_START_TEST_ROOT: &str = "CLANTODESK_CONFIG_IMPORT_TEST_ROOT";
    #[cfg(target_os = "linux")]
    const SERVICE_START_TEST_PHASE: &str = "CLANTODESK_CONFIG_IMPORT_TEST_PHASE";
    #[cfg(target_os = "linux")]
    const PROFILE_CONFIG_PATH_FILE: &str = "profile-config-path";

    #[cfg(target_os = "linux")]
    struct FixedMachineKey([u8; 32]);

    #[cfg(target_os = "linux")]
    impl KeyProvider for FixedMachineKey {
        fn key(&self, scope: KeyScope) -> Option<[u8; 32]> {
            (scope == KeyScope::Machine).then_some(self.0)
        }
    }

    #[derive(Serialize)]
    struct LegacyConfig {
        id: String,
        enc_id: String,
        password: String,
        salt: String,
        key_pair: (Vec<u8>, Vec<u8>),
    }

    #[derive(Serialize)]
    struct LegacyConfig2 {
        unlock_pin: String,
        trusted_devices: String,
        options: HashMap<String, String>,
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "clantodesk-config-import-{}-{}",
                std::process::id(),
                thread_rng().gen::<u64>()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn legacy(value: &str) -> String {
        encrypt_str_or_original(value, "00", 1024 * 1024)
    }

    fn legacy_config(id: &str, password: &str, salt: &str, secret_key: &[u8]) -> LegacyConfig {
        LegacyConfig {
            id: String::new(),
            enc_id: legacy(id),
            password: legacy(password),
            salt: salt.to_owned(),
            key_pair: (
                encrypt_vec_or_original(secret_key, "00", 1024),
                format!("{id}-public-key").into_bytes(),
            ),
        }
    }

    fn legacy_config2(unlock_pin: &str, trusted_devices: &str) -> LegacyConfig2 {
        LegacyConfig2 {
            unlock_pin: legacy(unlock_pin),
            trusted_devices: legacy(trusted_devices),
            options: HashMap::from([
                ("2fa".to_owned(), format!("{unlock_pin}-totp")),
                ("host-option".to_owned(), format!("{unlock_pin}-option")),
            ]),
        }
    }

    fn write_toml(path: &Path, value: &impl Serialize) {
        fs::write(path, toml::to_string(value).unwrap()).unwrap();
    }

    fn read_toml(path: &Path) -> toml::Value {
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn backup_path(path: &Path) -> PathBuf {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".pre-import");
        backup.into()
    }

    fn decrypt_legacy(value: &toml::Value) -> String {
        let (plain, decrypted, _) = decrypt_str_or_original(value.as_str().unwrap(), "00");
        assert!(decrypted);
        plain
    }

    fn paths(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let source = root.join("profile/ClantoDesk.toml");
        let source2 = root.join("profile/ClantoDesk2.toml");
        let destination = root.join("service/ClantoDesk.toml");
        let destination2 = root.join("service/ClantoDesk2.toml");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        (source, source2, destination, destination2)
    }

    #[test]
    fn import_decision_requires_all_three_conditions() {
        for destination_has_identity in [false, true] {
            for source_newer in [false, true] {
                for source_older_than_exe in [false, true] {
                    assert_eq!(
                        should_import_config(
                            destination_has_identity,
                            source_newer,
                            source_older_than_exe,
                        ),
                        !destination_has_identity && source_newer && source_older_than_exe
                    );
                }
            }
        }

        let root = TestDirectory::new();
        let destination = root.0.join("ClantoDesk.toml");
        for config in [
            "id = \"123456789\"\n",
            "enc_id = \"00encrypted\"\n",
            "key_pair = [[1], []]\n",
        ] {
            fs::write(&destination, config).unwrap();
            assert!(destination_has_identity(&destination).unwrap());
        }
        fs::write(
            &destination,
            "id = \"\"\nenc_id = \"\"\nkey_pair = [[], []]\n",
        )
        .unwrap();
        assert!(!destination_has_identity(&destination).unwrap());
    }

    #[test]
    fn first_install_imports_profile_and_backs_up_only_existing_files() {
        let root = TestDirectory::new();
        let (source, source2, destination, destination2) = paths(&root.0);
        write_toml(
            &source,
            &legacy_config(
                "profile-id",
                "profile-password",
                "profile-salt",
                b"profile-secret",
            ),
        );
        write_toml(
            &source2,
            &legacy_config2("333333", r#"[{"id":"profile-device"}]"#),
        );
        #[cfg(not(windows))]
        fs::write(source.parent().unwrap().join("machine.key"), [0x22; 32]).unwrap();

        import_config_files(
            &source,
            &source2,
            &destination,
            &destination2,
            true,
            true,
            true,
        )
        .unwrap();

        let imported = read_toml(&destination);
        assert_eq!(decrypt_legacy(&imported["enc_id"]), "profile-id");
        assert_eq!(decrypt_legacy(&imported["password"]), "profile-password");
        assert!(!backup_path(&destination).exists());
        assert!(!backup_path(&destination2).exists());
        #[cfg(not(windows))]
        assert_eq!(
            fs::read(destination.parent().unwrap().join("machine.key")).unwrap(),
            [0x22; 32]
        );

        let empty_destination = root.0.join("empty-service/ClantoDesk.toml");
        let empty_destination2 = root.0.join("empty-service/ClantoDesk2.toml");
        fs::create_dir_all(empty_destination.parent().unwrap()).unwrap();
        let empty_config = "id = \"\"\nenc_id = \"\"\nkey_pair = [[], []]\n";
        let empty_config2 = "unlock_pin = \"\"\ntrusted_devices = \"\"\n";
        fs::write(&empty_destination, empty_config).unwrap();
        fs::write(&empty_destination2, empty_config2).unwrap();

        import_config_files(
            &source,
            &source2,
            &empty_destination,
            &empty_destination2,
            true,
            true,
            true,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(backup_path(&empty_destination)).unwrap(),
            empty_config
        );
        assert_eq!(
            fs::read_to_string(backup_path(&empty_destination2)).unwrap(),
            empty_config2
        );
    }

    #[test]
    fn existing_identity_blocks_newer_profile_without_backups_or_key_copy() {
        let root = TestDirectory::new();
        let (source, source2, destination, destination2) = paths(&root.0);
        write_toml(
            &source,
            &legacy_config(
                "profile-id",
                "profile-password",
                "profile-salt",
                b"profile-secret",
            ),
        );
        write_toml(&source2, &legacy_config2("222222", "[]"));
        write_toml(
            &destination,
            &legacy_config(
                "service-id",
                "service-password",
                "service-salt",
                b"service-secret",
            ),
        );
        write_toml(&destination2, &legacy_config2("111111", "[]"));
        let destination_before = fs::read(&destination).unwrap();
        let destination2_before = fs::read(&destination2).unwrap();
        let source_key = source.parent().unwrap().join("machine.key");
        let destination_key = destination.parent().unwrap().join("machine.key");
        fs::write(&source_key, [0x22; 32]).unwrap();
        fs::write(&destination_key, [0x11; 32]).unwrap();

        import_config_files(
            &source,
            &source2,
            &destination,
            &destination2,
            true,
            true,
            true,
        )
        .unwrap();

        assert_eq!(fs::read(&destination).unwrap(), destination_before);
        assert_eq!(fs::read(&destination2).unwrap(), destination2_before);
        assert_eq!(fs::read(&destination_key).unwrap(), [0x11; 32]);
        assert!(!backup_path(&destination).exists());
        assert!(!backup_path(&destination2).exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn upgrade_keeps_service_149_data_after_the_first_150_service_start() {
        if let (Some(root), Ok(phase)) = (
            std::env::var_os(SERVICE_START_TEST_ROOT),
            std::env::var(SERVICE_START_TEST_PHASE),
        ) {
            match phase.as_str() {
                "profile" => run_profile_migration(Path::new(&root)),
                "installer" => run_installer_import_sequence(Path::new(&root)),
                "service" => assert_first_service_start(Path::new(&root)),
                _ => panic!("unexpected test phase"),
            }
            return;
        }

        let root = TestDirectory::new();
        for phase in ["profile", "installer", "service"] {
            let xdg_config = if phase == "profile" {
                root.0.join("profile-xdg")
            } else {
                root.0.join("service-xdg")
            };
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg(
                    "clanto::config_import::tests::upgrade_keeps_service_149_data_after_the_first_150_service_start",
                )
                .arg("--nocapture")
                .env(SERVICE_START_TEST_ROOT, &root.0)
                .env(SERVICE_START_TEST_PHASE, phase)
                .env("XDG_CONFIG_HOME", xdg_config)
                .status()
                .unwrap();
            assert!(status.success(), "{phase} phase failed");
        }
    }

    #[cfg(target_os = "linux")]
    fn run_profile_migration(root: &Path) {
        *hbb_common::config::APP_NAME.write().unwrap() = "ClantoDesk".to_owned();
        let source = Config::file();
        let source2 = Config2::file();
        let relative_config = source.strip_prefix(root.join("profile-xdg")).unwrap();
        let destination = root.join("service-xdg").join(relative_config);
        let destination2 = PathBuf::from(destination.to_string_lossy().replace(".toml", "2.toml"));
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let service_devices = r#"[{"id":"service-device"}]"#;
        write_toml(
            &source,
            &legacy_config(
                "profile-id",
                "profile-password",
                "profile-salt",
                b"profile-secret",
            ),
        );
        write_toml(
            &source2,
            &legacy_config2("222222", r#"[{"id":"profile-device"}]"#),
        );
        write_toml(
            &destination,
            &legacy_config(
                "service-id",
                "service-password",
                "service-salt",
                b"service-secret",
            ),
        );
        write_toml(&destination2, &legacy_config2("111111", service_devices));
        fs::write(source.parent().unwrap().join("machine.key"), [0x22; 32]).unwrap();
        fs::write(
            destination.parent().unwrap().join("machine.key"),
            [0x11; 32],
        )
        .unwrap();
        fs::write(
            root.join(PROFILE_CONFIG_PATH_FILE),
            source.as_os_str().as_encoded_bytes(),
        )
        .unwrap();

        set_key_provider(Arc::new(FixedMachineKey([0x22; 32])));
        assert_eq!(Config::get_id(), "profile-id");
        assert_eq!(Config::get_unlock_pin(), "222222");
        let migrated = read_toml(&source);
        let migrated2 = read_toml(&source2);
        assert!(migrated["enc_id"].as_str().unwrap().starts_with("02"));
        assert!(migrated["password"].as_str().unwrap().starts_with("02"));
        assert!(migrated2["unlock_pin"].as_str().unwrap().starts_with("02"));
    }

    #[cfg(target_os = "linux")]
    fn run_installer_import_sequence(root: &Path) {
        *hbb_common::config::APP_NAME.write().unwrap() = "ClantoDesk".to_owned();
        let source =
            PathBuf::from(fs::read_to_string(root.join(PROFILE_CONFIG_PATH_FILE)).unwrap());
        let source2 = PathBuf::from(source.to_string_lossy().replace(".toml", "2.toml"));
        let destination = Config::file();
        let destination2 = Config2::file();

        import_config_files(
            &source,
            &source2,
            &destination,
            &destination2,
            true,
            true,
            true,
        )
        .unwrap();
    }

    #[cfg(target_os = "linux")]
    fn assert_first_service_start(root: &Path) {
        *hbb_common::config::APP_NAME.write().unwrap() = "ClantoDesk".to_owned();
        let destination = Config::file();
        let destination2 = Config2::file();
        assert!(destination.starts_with(root.join("service-xdg")));
        let service_devices = r#"[{"id":"service-device"}]"#;
        set_key_provider(Arc::new(FixedMachineKey([0x11; 32])));
        assert_eq!(Config::get_id(), "service-id");
        assert_eq!(
            Config::get_key_pair(),
            (
                b"service-secret".to_vec(),
                b"service-id-public-key".to_vec()
            )
        );
        let (password, salt) = Config::get_local_permanent_password_storage_and_salt();
        assert_eq!(password, "service-password");
        assert_eq!(salt, "service-salt");
        assert_eq!(Config::get_unlock_pin(), "111111");

        let stored = read_toml(&destination);
        let stored2 = read_toml(&destination2);
        let (stored_id, id_decrypted, _) =
            decrypt_str_or_original_scoped(stored["enc_id"].as_str().unwrap(), KeyScope::Machine);
        let (stored_devices, devices_decrypted, _) = decrypt_str_or_original_scoped(
            stored2["trusted_devices"].as_str().unwrap(),
            KeyScope::Machine,
        );
        assert!(stored["enc_id"].as_str().unwrap().starts_with("02"));
        assert!(stored["password"].as_str().unwrap().starts_with("02"));
        assert_eq!(
            stored["key_pair"][0]
                .as_array()
                .unwrap()
                .iter()
                .take(2)
                .map(|value| value.as_integer().unwrap() as u8)
                .collect::<Vec<_>>(),
            b"02"
        );
        assert!(stored2["unlock_pin"].as_str().unwrap().starts_with("02"));
        assert!(stored2["trusted_devices"]
            .as_str()
            .unwrap()
            .starts_with("02"));
        assert!(id_decrypted);
        assert!(devices_decrypted);
        assert_eq!(stored_id, "service-id");
        assert_eq!(stored_devices, service_devices);
        assert_eq!(stored2["options"]["2fa"].as_str(), Some("111111-totp"));
        assert_eq!(
            stored2["options"]["host-option"].as_str(),
            Some("111111-option")
        );
        assert_eq!(
            fs::read(destination.parent().unwrap().join("machine.key")).unwrap(),
            [0x11; 32]
        );
    }
}
