//! Credential values never enter the plugin registry or logs.
use std::{collections::BTreeMap, sync::Mutex};

static FILE_BACKEND: std::sync::OnceLock<FileBackend> = std::sync::OnceLock::new();

/// Called once, before app initialization; no environment or OS credential reads.
pub(crate) fn initialize_from_args() -> Result<(), String> {
    if let Some(path) = credentials_path(std::env::args_os().skip(1))? {
        let backend = FileBackend::new(path)?;
        FILE_BACKEND.set(backend).map_err(|_| "Credential backend already initialized")?;
    }
    Ok(())
}

fn credentials_path(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Option<std::path::PathBuf>, String> {
    let mut args = args.into_iter();
    let mut path = None;
    while let Some(arg) = args.next() {
        if arg == "--credentials-file" {
            if path.is_some() {
                return Err("Duplicate --credentials-file argument".into());
            }
            let value = args.next().filter(|v| !v.is_empty() && !v.to_string_lossy().starts_with("--"))
                .ok_or("--credentials-file requires a path")?;
            path = Some(value.into());
        } else if arg.to_string_lossy().starts_with("--credentials-file=") {
            return Err("Use --credentials-file <path>".into());
        }
    }
    Ok(path)
}

struct FileBackend {
    path: std::path::PathBuf,
    lock: Mutex<()>,
}

impl FileBackend {
    fn new(path: std::path::PathBuf) -> Result<Self, String> {
        let path = if path.is_absolute() { path } else {
            std::env::current_dir().map_err(|_| "Cannot resolve credentials path")?.join(path)
        };
        let parent = path.parent().ok_or("Invalid credentials file path")?;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent).map_err(|_| "Cannot create credentials directory")?;
        let backend = Self { path, lock: Mutex::new(()) };
        let _guard = backend.lock.lock().map_err(|_| "Credential file unavailable")?;
        backend.load()?;
        drop(_guard);
        if !backend.path.exists() {
            backend.update(String::new(), None)?;
        }
        Ok(backend)
    }

    fn load(&self) -> Result<BTreeMap<String, String>, String> {
        use std::io::Read;
        match std::fs::symlink_metadata(&self.path) {
            Ok(meta) if !meta.file_type().is_file() => return Err("Credentials path must be a regular file, not a symlink".into()),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(_) => return Err("Cannot inspect credentials file".into()),
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // O_NOFOLLOW prevents a symlink swap between inspection and open.
            #[cfg(target_os = "macos")]
            options.custom_flags(0x100);
            #[cfg(any(target_os = "linux", target_os = "android"))]
            options.custom_flags(0x20000);
        }
        let mut file = options.open(&self.path).map_err(|_| "Cannot open credentials file safely")?;
        if !file.metadata().map_err(|_| "Cannot inspect credentials file")?.is_file() {
            return Err("Credentials path must be a regular file".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|_| "Cannot secure credentials file permissions")?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|_| "Cannot read credentials file")?;
        serde_json::from_slice(&bytes).map_err(|_| "Credentials file must contain a JSON object of secret strings".into())
    }

    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let _guard = self.lock.lock().map_err(|_| "Credential file unavailable")?;
        Ok(self.load()?.remove(key))
    }

    fn update(&self, key: String, value: Option<&str>) -> Result<(), String> {
        use std::io::Write;
        let _guard = self.lock.lock().map_err(|_| "Credential file unavailable")?;
        let mut values = self.load()?;
        if let Some(value) = value { values.insert(key, value.into()); } else { values.remove(&key); }
        let bytes = serde_json::to_vec(&values).map_err(|_| "Cannot encode credentials file")?;
        let parent = self.path.parent().ok_or("Invalid credentials file path")?;
        let (temporary, mut file) = loop {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let temporary = parent.join(format!(".player-credentials-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&temporary) {
                Ok(file) => break (temporary, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err("Cannot create credentials replacement file".into()),
            }
        };
        let result = (|| {
            file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| "Cannot persist credentials file")?;
            std::fs::rename(&temporary, &self.path).map_err(|_| "Cannot replace credentials file")?;
            Ok(())
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temporary); }
        result
    }
}

/// Successful reads, missing values and refusals are shared within a plugin activation.
/// Holding the lock across provider access coalesces concurrent reads into one OS request.
#[derive(Default)]
pub struct SecretCache {
    values: Mutex<BTreeMap<String, Result<Option<String>, String>>>,
}
impl SecretCache {
    fn get_with(
        &self,
        key: String,
        read: impl FnOnce() -> Result<Option<String>, String>,
    ) -> Result<Option<String>, String> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| "Credential cache is unavailable")?;
        values.entry(key).or_insert_with(read).clone()
    }
    fn update_with(
        &self,
        key: String,
        value: Option<String>,
        write: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| "Credential cache is unavailable")?;
        write()?;
        values.insert(key, Ok(value));
        Ok(())
    }
    pub fn get(&self, id: &str, name: &str) -> Result<Option<String>, String> {
        self.get_with(key(id, name)?, || read(id, name))
    }
    pub fn set(&self, id: &str, name: &str, value: &str) -> Result<(), String> {
        if value.len() > 16384 {
            return Err("Secret too large".into());
        }
        self.update_with(key(id, name)?, Some(value.into()), || {
            write(id, name, value)
        })
    }
    pub fn delete(&self, id: &str, name: &str) -> Result<(), String> {
        self.update_with(key(id, name)?, None, || remove(id, name))
    }
    pub fn forget_plugin(&self, id: &str) -> Result<(), String> {
        let prefix = format!("{id}/");
        self.values
            .lock()
            .map_err(|_| "Credential cache is unavailable")?
            .retain(|key, _| !key.starts_with(&prefix));
        Ok(())
    }
}
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
fn key(id: &str, name: &str) -> Result<String, String> {
    if !super::model::valid_local_id(name) {
        return Err("Invalid secret key".into());
    }
    Ok(format!("{id}/{name}"))
}
#[cfg(target_os = "windows")]
fn wide(key: &str) -> Vec<u16> {
    format!("dev.verncat.player.plugins/{key}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}
fn write(id: &str, name: &str, value: &str) -> Result<(), String> {
    if let Some(backend) = FILE_BACKEND.get() {
        return backend.update(key(id, name)?, Some(value));
    }
    let key = key(id, name)?;
    if value.len() > 16384 {
        return Err("Secret too large".into());
    }
    #[cfg(target_os = "macos")]
    {
        return security_framework::passwords::set_generic_password(
            "dev.verncat.player.plugins",
            &key,
            value.as_bytes(),
        )
        .map_err(|e| format!("Keychain: {e}"));
    }
    #[cfg(target_os = "linux")]
    {
        let mut child = Command::new("secret-tool")
            .args([
                "store",
                "--label=Player plugin",
                "application",
                "dev.verncat.player",
                "key",
                &key,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                "Install a Secret Service provider and secret-tool to save plugin credentials"
            })?;
        child
            .stdin
            .take()
            .ok_or("Secret Service stdin unavailable")?
            .write_all(value.as_bytes())
            .map_err(|e| e.to_string())?;
        return if child.wait().map_err(|e| e.to_string())?.success() {
            Ok(())
        } else {
            Err("Secret Service refused credential storage".into())
        };
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Security::Credentials::*;
        let mut target = wide(&key);
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: value.len() as u32,
            CredentialBlob: value.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        return if unsafe { CredWriteW(&credential, 0) } != 0 {
            Ok(())
        } else {
            Err(format!(
                "Credential Manager: {}",
                std::io::Error::last_os_error()
            ))
        };
    }
    #[cfg(target_os = "android")]
    {
        return mobile_call("write", serde_json::json!({"key":key,"value":value})).map(|_| ());
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        let _ = (key, value);
        Err("Secure credential provider is unavailable on this platform".into())
    }
}
fn read(id: &str, name: &str) -> Result<Option<String>, String> {
    if let Some(backend) = FILE_BACKEND.get() {
        return backend.get(&key(id, name)?);
    }
    let key = key(id, name)?;
    #[cfg(target_os = "macos")]
    {
        match security_framework::passwords::get_generic_password(
            "dev.verncat.player.plugins",
            &key,
        ) {
            Ok(v) => return String::from_utf8(v).map(Some).map_err(|e| e.to_string()),
            Err(e) if e.code() == -25300 => return Ok(None),
            Err(e) => return Err(format!("Keychain: {e}")),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("secret-tool")
            .args(["lookup", "application", "dev.verncat.player", "key", &key])
            .stderr(Stdio::null())
            .output()
            .map_err(|_| "Secret Service is unavailable")?;
        return if output.status.success() {
            String::from_utf8(output.stdout)
                .map(|s| Some(s.trim_end_matches('\n').into()))
                .map_err(|e| e.to_string())
        } else {
            Ok(None)
        };
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Security::Credentials::*;
        let target = wide(&key);
        let mut credential = std::ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(1168) {
                Ok(None)
            } else {
                Err(format!("Credential Manager: {error}"))
            };
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (*credential).CredentialBlob,
                (*credential).CredentialBlobSize as usize,
            )
        }
        .to_vec();
        unsafe { CredFree(credential.cast()) };
        return String::from_utf8(bytes)
            .map(Some)
            .map_err(|e| e.to_string());
    }
    #[cfg(target_os = "android")]
    {
        return mobile_call("read", serde_json::json!({"key":key}))
            .and_then(|v| serde_json::from_value(v["value"].clone()).map_err(|e| e.to_string()));
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        let _ = key;
        Err("Secure credential provider is unavailable on this platform".into())
    }
}
fn remove(id: &str, name: &str) -> Result<(), String> {
    if let Some(backend) = FILE_BACKEND.get() {
        return backend.update(key(id, name)?, None);
    }
    let key = key(id, name)?;
    #[cfg(target_os = "macos")]
    {
        return match security_framework::passwords::delete_generic_password(
            "dev.verncat.player.plugins",
            &key,
        ) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(format!("Keychain: {e}")),
        };
    }
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("secret-tool")
            .args(["clear", "application", "dev.verncat.player", "key", &key])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "Secret Service is unavailable")?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Security::Credentials::*;
        let target = wide(&key);
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        return if e.raw_os_error() == Some(1168) {
            Ok(())
        } else {
            Err(format!("Credential Manager: {e}"))
        };
    }
    #[cfg(target_os = "android")]
    {
        return mobile_call("delete", serde_json::json!({"key":key})).map(|_| ());
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "linux",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        let _ = key;
        Err("Secure credential provider is unavailable on this platform".into())
    }
}

#[cfg(target_os = "android")]
static MOBILE: std::sync::OnceLock<tauri::plugin::PluginHandle<tauri::Wry>> =
    std::sync::OnceLock::new();
#[cfg(target_os = "android")]
fn mobile_call(method: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    MOBILE
        .get()
        .ok_or("Android credential provider not initialized")?
        .run_mobile_plugin(method, args)
        .map_err(|e| e.to_string())
}
#[cfg(target_os = "android")]
pub fn mobile_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("player-secrets")
        .setup(|_app, api| {
            let handle = api.register_android_plugin("dev.verncat.player", "PluginSecrets")?;
            let _ = MOBILE.set(handle);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    #[test]
    fn file_persistence_delete_isolation_malformed_and_modes() {
        let root = std::env::temp_dir().join(format!("player-secret-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("private/credentials.json");
        let backend = FileBackend::new(path.clone()).unwrap();
        backend.update("one/password".into(), Some("dummy-only")).unwrap();
        backend.update("two/password".into(), Some("other-dummy")).unwrap();
        let reopened = FileBackend::new(path.clone()).unwrap();
        assert_eq!(reopened.get("one/password").unwrap().as_deref(), Some("dummy-only"));
        reopened.update("one/password".into(), None).unwrap();
        assert_eq!(backend.get("one/password").unwrap(), None);
        assert_eq!(backend.get("two/password").unwrap().as_deref(), Some("other-dummy"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            backend.get("two/password").unwrap();
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            let link = root.join("linked.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert!(FileBackend::new(link).is_err());
        }
        std::fs::write(&path, b"not json").unwrap();
        assert!(FileBackend::new(path.clone()).is_err());
        std::fs::write(&path, br#"{"one/password":123}"#).unwrap();
        assert!(FileBackend::new(path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credentials_launch_argument_validation() {
        let parse = |args: &[&str]| credentials_path(args.iter().map(std::ffi::OsString::from));
        assert_eq!(parse(&[]).unwrap(), None);
        assert_eq!(parse(&["--credentials-file", "relative.json"]).unwrap(), Some("relative.json".into()));
        assert!(parse(&["--credentials-file"]).is_err());
        assert!(parse(&["--credentials-file", "--demo"]).is_err());
        assert!(parse(&["--credentials-file", "a", "--credentials-file", "b"]).is_err());
    }

    #[test]
    fn file_mode_internal_api_smoke_without_os_credentials() {
        let root = std::env::temp_dir().join(format!("player-secret-api-test-{}", std::process::id()));
        let path = root.join("credentials.json");
        FILE_BACKEND.set(FileBackend::new(path.clone()).unwrap()).ok().unwrap();
        let cache = SecretCache::default();
        cache.set("test.filemode", "password", "temporary-dummy-not-a-password").unwrap();
        assert_eq!(cache.get("test.filemode", "password").unwrap().as_deref(), Some("temporary-dummy-not-a-password"));
        cache.forget_plugin("test.filemode").unwrap();
        assert_eq!(cache.get("test.filemode", "password").unwrap().as_deref(), Some("temporary-dummy-not-a-password"));
        cache.delete("test.filemode", "password").unwrap();
        cache.forget_plugin("test.filemode").unwrap();
        assert_eq!(cache.get("test.filemode", "password").unwrap(), None);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_reads_only_open_the_credential_provider_once() {
        let cache = Arc::new(SecretCache::default());
        let count = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let cache = cache.clone();
                let count = count.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    cache
                        .get_with("example.one/password".into(), || {
                            count.fetch_add(1, Ordering::SeqCst);
                            Ok(Some("test-only".into()))
                        })
                        .unwrap()
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().unwrap().as_deref(), Some("test-only"));
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn refusal_is_not_retried_automatically_and_reset_is_scoped_to_plugin() {
        let cache = SecretCache::default();
        let count = AtomicUsize::new(0);
        for _ in 0..5 {
            assert!(cache
                .get_with("example.one/password".into(), || {
                    count.fetch_add(1, Ordering::SeqCst);
                    Err("Access declined".into())
                })
                .is_err());
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        cache
            .get_with("example.two/password".into(), || Ok(Some("other".into())))
            .unwrap();
        cache.forget_plugin("example.one").unwrap();
        assert_eq!(
            cache
                .get_with("example.one/password".into(), || {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(Some("retry".into()))
                })
                .unwrap()
                .as_deref(),
            Some("retry")
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(
            cache
                .get_with("example.two/password".into(), || panic!(
                    "unrelated cache was cleared"
                ))
                .unwrap()
                .as_deref(),
            Some("other")
        );
    }

    #[test]
    fn writes_and_deletes_update_cache_only_after_success() {
        let cache = SecretCache::default();
        assert!(cache
            .set("example.one", "password", &"a".repeat(16385))
            .is_err());
        let key = "example.one/password".to_string();
        cache
            .get_with(key.clone(), || Err("Access declined".into()))
            .unwrap_err();
        cache
            .update_with(key.clone(), Some("saved".into()), || Ok(()))
            .unwrap();
        assert_eq!(
            cache
                .get_with(key.clone(), || panic!("save should update cache"))
                .unwrap()
                .as_deref(),
            Some("saved")
        );
        assert!(cache
            .update_with(key.clone(), Some("failed".into()), || Err(
                "write failed".into()
            ))
            .is_err());
        assert_eq!(
            cache
                .get_with(key.clone(), || panic!("failed write should keep old value"))
                .unwrap()
                .as_deref(),
            Some("saved")
        );
        cache.update_with(key.clone(), None, || Ok(())).unwrap();
        assert_eq!(
            cache
                .get_with(key, || panic!("delete should cache absence"))
                .unwrap(),
            None
        );
    }
}
