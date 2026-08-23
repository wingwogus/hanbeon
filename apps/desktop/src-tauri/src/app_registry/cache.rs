use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::http::{Validators, read_limited, verify_sha256};
use super::model::{
    AppEntry, AppProfile, INDEX_LIMIT, PROFILE_LIMIT, RegistryIndex, parse_index, parse_profile,
};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Cache {
    root: PathBuf,
}

pub(crate) struct CachedIndex {
    pub(crate) index: RegistryIndex,
    pub(crate) sha256: String,
    pub(crate) validators: Validators,
}

pub(crate) struct CachedProfile {
    pub(crate) sha256: String,
    pub(crate) profile: AppProfile,
}

impl Cache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn store_index(
        &self,
        raw: &[u8],
        sha256: &str,
        validators: &Validators,
    ) -> Result<RegistryIndex, String> {
        verify_sha256(raw, sha256)?;
        let index = parse_index(raw)?;

        if let Some(current) = self.load_latest_index()? {
            if index.revision < current.index.revision {
                return Err(format!(
                    "registry revision을 {}에서 {}로 되돌릴 수 없습니다.",
                    current.index.revision, index.revision
                ));
            }
            if index.revision == current.index.revision && sha256 != current.sha256 {
                return Err(format!(
                    "같은 registry revision {}에 서로 다른 내용이 있습니다.",
                    index.revision
                ));
            }
        }

        let directory = self.indexes_dir();
        let index_path = directory.join(format!("{sha256}.json"));
        write_immutable(&index_path, raw)?;

        let headers = serde_json::to_vec(validators)
            .map_err(|error| format!("registry validator를 직렬화하지 못했습니다. ({error})"))?;
        let headers_path = directory.join(format!("{sha256}.headers.json"));
        if !headers_path.exists() {
            write_immutable(&headers_path, &headers)?;
        }

        Ok(index)
    }

    pub(crate) fn load_latest_index(&self) -> Result<Option<CachedIndex>, String> {
        Ok(self.load_valid_indexes()?.into_iter().next())
    }

    fn load_valid_indexes(&self) -> Result<Vec<CachedIndex>, String> {
        let entries = match fs::read_dir(self.indexes_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!("registry cache 목록을 읽지 못했습니다. ({error})"));
            }
        };

        let mut indexes = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(sha256) = cached_hash(&path) else {
                continue;
            };
            let Ok(raw) = read_cache_file(&path, INDEX_LIMIT) else {
                continue;
            };
            if verify_sha256(&raw, sha256).is_err() {
                continue;
            }
            let Ok(index) = parse_index(&raw) else {
                continue;
            };
            let validators = read_validators(&self.indexes_dir(), sha256);
            indexes.push(CachedIndex {
                index,
                sha256: sha256.to_string(),
                validators,
            });
        }

        indexes.sort_by(|left, right| {
            right
                .index
                .revision
                .cmp(&left.index.revision)
                .then_with(|| right.sha256.cmp(&left.sha256))
        });
        Ok(indexes)
    }

    pub(crate) fn store_profile(&self, entry: &AppEntry, raw: &[u8]) -> Result<AppProfile, String> {
        verify_sha256(raw, &entry.sha256)?;
        let profile = parse_profile(raw, &entry.id)?;
        let path = self.profiles_dir().join(format!("{}.json", entry.sha256));
        write_immutable(&path, raw)?;
        Ok(profile)
    }

    pub(crate) fn load_profile(&self, entry: &AppEntry) -> Option<AppProfile> {
        let path = self.profiles_dir().join(format!("{}.json", entry.sha256));
        let raw = read_cache_file(&path, PROFILE_LIMIT).ok()?;
        verify_sha256(&raw, &entry.sha256).ok()?;
        parse_profile(&raw, &entry.id).ok()
    }

    pub(crate) fn load_fallback_profile(&self, app_id: &str) -> Option<CachedProfile> {
        for cached in self.load_valid_indexes().ok()? {
            let Some(entry) = cached.index.apps.iter().find(|entry| entry.id == app_id) else {
                continue;
            };
            if let Some(profile) = self.load_profile(entry) {
                return Some(CachedProfile {
                    sha256: entry.sha256.clone(),
                    profile,
                });
            }
        }
        None
    }

    fn indexes_dir(&self) -> PathBuf {
        self.root.join("indexes")
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }
}

fn cached_hash(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let sha256 = name.strip_suffix(".json")?;
    (sha256.len() == 64
        && sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(sha256)
}

fn read_cache_file(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file =
        File::open(path).map_err(|error| format!("registry cache를 열지 못했습니다. ({error})"))?;
    read_limited(file, limit)
}

fn read_validators(directory: &Path, sha256: &str) -> Validators {
    let path = directory.join(format!("{sha256}.headers.json"));
    read_cache_file(&path, 8 * 1024)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn write_immutable(path: &Path, raw: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "registry cache 경로에 상위 폴더가 없습니다.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("registry cache 폴더를 만들지 못했습니다. ({error})"))?;

    if path.exists() {
        return verify_existing(path, raw);
    }

    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "registry cache 파일명이 잘못됐습니다.".to_string())?;
    let temporary = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("registry 임시 cache를 만들지 못했습니다. ({error})"))?;

    let result = (|| {
        file.write_all(raw)
            .map_err(|error| format!("registry cache를 쓰지 못했습니다. ({error})"))?;
        file.sync_all()
            .map_err(|error| format!("registry cache를 동기화하지 못했습니다. ({error})"))?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|error| format!("registry cache를 확정하지 못했습니다. ({error})"))
    })();

    if result.is_err() && path.exists() && verify_existing(path, raw).is_ok() {
        let _ = fs::remove_file(&temporary);
        return Ok(());
    }
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn verify_existing(path: &Path, expected: &[u8]) -> Result<(), String> {
    let actual = read_cache_file(path, expected.len())?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "기존 registry cache 내용이 다릅니다. ({})",
            path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::app_registry::http::sha256_hex;
    use crate::app_registry::model::parse_index;

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "hanbeon-hana-cloud-cache-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profile_raw() -> Vec<u8> {
        r#"{
            "schemaVersion": 1,
            "id": "pdf-viewer",
            "actions": [{
                "label": "다음 장",
                "name": "페이지 넘기기",
                "shortcut": {
                    "macos": "pagedown",
                    "windows": "pagedown",
                    "linux": "pagedown"
                }
            }]
        }"#
        .as_bytes()
        .to_vec()
    }

    fn index_raw(revision: u64, profile_sha256: &str) -> Vec<u8> {
        format!(
            r#"{{
                "schemaVersion": 1,
                "revision": {revision},
                "apps": [{{
                    "id": "pdf-viewer",
                    "name": "PDF 뷰어",
                    "path": "apps/pdf-viewer.json",
                    "sha256": "{profile_sha256}",
                    "match": {{
                        "windows": {{ "executables": ["Acrobat.exe"] }}
                    }}
                }}],
                "boards": []
            }}"#
        )
        .into_bytes()
    }

    fn entry_for(raw: &[u8]) -> AppEntry {
        parse_index(raw).unwrap().apps.remove(0)
    }

    #[test]
    fn stores_and_loads_a_hash_verified_profile() {
        let directory = TestDirectory::new();
        let cache = Cache::new(directory.0.clone());
        let profile = profile_raw();
        let profile_sha256 = sha256_hex(&profile);
        let index = index_raw(1, &profile_sha256);
        let entry = entry_for(&index);

        assert_eq!(
            cache.store_profile(&entry, &profile).unwrap().id,
            "pdf-viewer"
        );
        assert_eq!(cache.load_profile(&entry).unwrap().id, "pdf-viewer");
        assert!(
            cache
                .profiles_dir()
                .join(format!("{profile_sha256}.json"))
                .is_file()
        );
    }

    #[test]
    fn rejects_a_profile_whose_bytes_do_not_match_the_index_hash() {
        let directory = TestDirectory::new();
        let cache = Cache::new(directory.0.clone());
        let index = index_raw(1, &"0".repeat(64));
        let entry = entry_for(&index);

        assert!(cache.store_profile(&entry, &profile_raw()).is_err());
        assert_eq!(cache.load_profile(&entry), None);
    }

    #[test]
    fn loads_the_highest_valid_revision_and_its_validators() {
        let directory = TestDirectory::new();
        let cache = Cache::new(directory.0.clone());
        let profile_sha256 = sha256_hex(&profile_raw());
        let older = index_raw(2, &profile_sha256);
        let newer = index_raw(3, &profile_sha256);
        let newer_validators = Validators {
            etag: Some("\"revision-3\"".into()),
            last_modified: Some("Sun, 23 Aug 2026 00:00:00 GMT".into()),
        };

        cache
            .store_index(&older, &sha256_hex(&older), &Validators::default())
            .unwrap();
        cache
            .store_index(&newer, &sha256_hex(&newer), &newer_validators)
            .unwrap();

        let loaded = cache.load_latest_index().unwrap().unwrap();
        assert_eq!(loaded.index.revision, 3);
        assert_eq!(loaded.sha256, sha256_hex(&newer));
        assert_eq!(loaded.validators, newer_validators);
    }

    #[test]
    fn falls_back_when_the_newest_cache_file_is_corrupt_and_ignores_temp_files() {
        let directory = TestDirectory::new();
        let cache = Cache::new(directory.0.clone());
        let profile_sha256 = sha256_hex(&profile_raw());
        let older = index_raw(4, &profile_sha256);
        cache
            .store_index(&older, &sha256_hex(&older), &Validators::default())
            .unwrap();

        fs::create_dir_all(cache.indexes_dir()).unwrap();
        fs::write(
            cache.indexes_dir().join(format!("{}.json", "f".repeat(64))),
            index_raw(99, &profile_sha256),
        )
        .unwrap();
        fs::write(cache.indexes_dir().join("unfinished.tmp"), b"partial").unwrap();

        let loaded = cache.load_latest_index().unwrap().unwrap();
        assert_eq!(loaded.index.revision, 4);
        assert_eq!(loaded.sha256, sha256_hex(&older));
    }

    #[test]
    fn index_store_rejects_a_mismatched_content_hash() {
        let directory = TestDirectory::new();
        let cache = Cache::new(directory.0.clone());
        let profile_sha256 = sha256_hex(&profile_raw());
        let raw = index_raw(1, &profile_sha256);

        assert!(
            cache
                .store_index(&raw, &"0".repeat(64), &Validators::default())
                .is_err()
        );
        assert!(cache.load_latest_index().unwrap().is_none());
    }

    #[test]
    fn finds_an_older_verified_profile_when_the_latest_index_points_to_a_missing_update() {
        let directory = TestDirectory::new();
        let cache = Cache::new(directory.0.clone());
        let profile = profile_raw();
        let old_sha256 = sha256_hex(&profile);
        let old_index = index_raw(1, &old_sha256);
        let old_entry = entry_for(&old_index);

        cache
            .store_index(&old_index, &sha256_hex(&old_index), &Validators::default())
            .unwrap();
        cache.store_profile(&old_entry, &profile).unwrap();

        let new_index = index_raw(2, &"0".repeat(64));
        cache
            .store_index(&new_index, &sha256_hex(&new_index), &Validators::default())
            .unwrap();

        let fallback = cache.load_fallback_profile("pdf-viewer").unwrap();
        assert_eq!(fallback.sha256, old_sha256);
        assert_eq!(fallback.profile.id, "pdf-viewer");
    }
}
