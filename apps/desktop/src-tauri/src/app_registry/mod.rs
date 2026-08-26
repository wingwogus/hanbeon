mod cache;
mod http;
mod model;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::focused_application::FocusedApplication;

use self::cache::Cache;
use self::http::{HttpClient, IndexFetch, Validators};
pub(crate) use self::model::ResolvedAction;
use self::model::{AppEntry, AppProfile, Platform, RegistryIndex, matches_application};

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
const REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FAILURE_BACKOFF: Duration = Duration::from_secs(5 * 60);
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
const COMMAND_CAPACITY: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegistryPreset {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) sha256: String,
    pub(crate) actions: Vec<ResolvedAction>,
}

#[derive(Clone)]
pub(crate) struct Registry {
    state: Arc<Mutex<RegistryState>>,
    sender: SyncSender<Command>,
}

#[derive(Default)]
struct RegistryState {
    index: Option<RegistryIndex>,
    profiles: HashMap<String, StoredProfile>,
    retry_after: HashMap<String, Instant>,
}

struct StoredProfile {
    sha256: String,
    profile: AppProfile,
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
#[derive(Clone, Debug)]
enum Command {
    FetchProfile(AppEntry),
}

#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
impl Registry {
    /// 안드로이드용: 네트워크 스레드 없이 항상 빈 인덱스를 돌려주는 핸들.
    #[cfg_attr(feature = "desktop", allow(dead_code))]
    /// rustls-platform-verifier의 JNI 초기화가 없는 환경에서 reqwest가
    /// 패닉하므로 당분간 프리셋 동기화는 데스크톱 전용이다.
    pub(crate) fn noop(_cache_dir: std::path::PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(RegistryState::default())),
            sender: {
                let (tx, _rx) = std::sync::mpsc::sync_channel(1);
                tx
            },
        }
    }

    pub(crate) fn spawn(cache_root: PathBuf) -> Self {
        let cache = Cache::new(cache_root);
        let cached = match cache.load_latest_index() {
            Ok(cached) => cached,
            Err(error) => {
                eprintln!("Hana Cloud cache를 읽지 못했습니다. {error}");
                None
            }
        };
        let (index, validators) = cached.map_or_else(
            || (None, Validators::default()),
            |cached| (Some(cached.index), cached.validators),
        );
        let profiles = index
            .as_ref()
            .map(|index| load_cached_profiles(&cache, index))
            .unwrap_or_default();

        let state = Arc::new(Mutex::new(RegistryState {
            index,
            profiles,
            retry_after: HashMap::new(),
        }));
        let (sender, receiver) = std::sync::mpsc::sync_channel(COMMAND_CAPACITY);

        let worker_state = Arc::clone(&state);
        thread::spawn(move || run_worker(cache, worker_state, receiver, validators));

        Self { state, sender }
    }

    pub(crate) fn lookup(&self, app: &FocusedApplication) -> Option<RegistryPreset> {
        let mut fetch = None;
        let preset = {
            let mut state = self.state.lock().ok()?;
            let entry = state
                .index
                .as_ref()?
                .apps
                .iter()
                .find(|entry| matches_application(entry, app))?
                .clone();

            let current_profile = state
                .profiles
                .get(&entry.id)
                .is_some_and(|stored| stored.sha256 == entry.sha256);
            let preset = state.profiles.get(&entry.id).and_then(|stored| {
                let actions = stored.profile.actions_for(Platform::current());
                (!actions.is_empty()).then(|| RegistryPreset {
                    id: entry.id.clone(),
                    name: entry.name.clone(),
                    sha256: stored.sha256.clone(),
                    actions,
                })
            });

            if !current_profile {
                let retry_key = format!("{}:{}", entry.id, entry.sha256);
                let now = Instant::now();
                if state
                    .retry_after
                    .get(&retry_key)
                    .is_none_or(|retry| *retry <= now)
                {
                    state
                        .retry_after
                        .insert(retry_key.clone(), now + FAILURE_BACKOFF);
                    fetch = Some((retry_key, entry));
                }
            }
            preset
        };

        if let Some((retry_key, entry)) = fetch
            && let Err(TrySendError::Full(_)) = self.sender.try_send(Command::FetchProfile(entry))
            && let Ok(mut state) = self.state.lock()
        {
            state
                .retry_after
                .insert(retry_key, Instant::now() + Duration::from_secs(1));
        }

        preset
    }

    #[cfg(test)]
    fn for_test(
        index: RegistryIndex,
        profiles: HashMap<String, StoredProfile>,
    ) -> (Self, Receiver<Command>) {
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        (
            Self {
                state: Arc::new(Mutex::new(RegistryState {
                    index: Some(index),
                    profiles,
                    retry_after: HashMap::new(),
                })),
                sender,
            },
            receiver,
        )
    }
}

fn load_cached_profiles(cache: &Cache, index: &RegistryIndex) -> HashMap<String, StoredProfile> {
    index
        .apps
        .iter()
        .filter_map(|entry| {
            cache
                .load_profile(entry)
                .map(|profile| (entry.sha256.clone(), profile))
                .or_else(|| {
                    cache
                        .load_fallback_profile(&entry.id)
                        .map(|cached| (cached.sha256, cached.profile))
                })
                .map(|(sha256, profile)| (entry.id.clone(), StoredProfile { sha256, profile }))
        })
        .collect()
}

fn run_worker(
    cache: Cache,
    state: Arc<Mutex<RegistryState>>,
    receiver: Receiver<Command>,
    mut validators: Validators,
) {
    let http = match HttpClient::new() {
        Ok(http) => http,
        Err(error) => {
            eprintln!("Hana Cloud HTTP client를 시작하지 못했습니다. {error}");
            return;
        }
    };
    let mut next_refresh = Instant::now();

    loop {
        let timeout = next_refresh.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(timeout) {
            Ok(Command::FetchProfile(entry)) => {
                if let Err(error) = fetch_profile(&http, &cache, &state, &entry) {
                    eprintln!("Hana Cloud app profile을 갱신하지 못했습니다. {error}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                let succeeded = refresh_index(&http, &cache, &state, &mut validators).is_ok();
                next_refresh = Instant::now()
                    + if succeeded {
                        REFRESH_INTERVAL
                    } else {
                        FAILURE_BACKOFF
                    };
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn refresh_index(
    http: &HttpClient,
    cache: &Cache,
    state: &Arc<Mutex<RegistryState>>,
    validators: &mut Validators,
) -> Result<(), String> {
    match http.fetch_index(validators)? {
        IndexFetch::NotModified => Ok(()),
        IndexFetch::Modified {
            raw,
            sha256,
            validators: next_validators,
        } => {
            let index = cache.store_index(&raw, &sha256, &next_validators)?;
            let profiles = load_cached_profiles(cache, &index);
            let mut current = state
                .lock()
                .map_err(|_| "registry 메모리 상태를 갱신하지 못했습니다.".to_string())?;
            current.index = Some(index);
            current.profiles = profiles;
            current.retry_after.clear();
            *validators = next_validators;
            Ok(())
        }
    }
}

fn fetch_profile(
    http: &HttpClient,
    cache: &Cache,
    state: &Arc<Mutex<RegistryState>>,
    entry: &AppEntry,
) -> Result<(), String> {
    let raw = http.fetch_profile(entry)?;
    let profile = cache.store_profile(entry, &raw)?;
    let mut current = state
        .lock()
        .map_err(|_| "registry 메모리 상태를 갱신하지 못했습니다.".to_string())?;
    let still_current = current.index.as_ref().is_some_and(|index| {
        index
            .apps
            .iter()
            .any(|candidate| candidate.id == entry.id && candidate.sha256 == entry.sha256)
    });
    if still_current {
        current.profiles.insert(
            entry.id.clone(),
            StoredProfile {
                sha256: entry.sha256.clone(),
                profile,
            },
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_registry::http::sha256_hex;
    use crate::app_registry::model::{parse_index, parse_profile};

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

    fn index_and_profile() -> (RegistryIndex, AppProfile, String) {
        let profile_raw = profile_raw();
        let sha256 = sha256_hex(&profile_raw);
        let index_raw = format!(
            r#"{{
                "schemaVersion": 1,
                "revision": 2,
                "apps": [{{
                    "id": "pdf-viewer",
                    "name": "PDF 뷰어",
                    "path": "apps/pdf-viewer.json",
                    "sha256": "{sha256}",
                    "match": {{
                        "windows": {{ "executables": ["Acrobat.exe"] }}
                    }}
                }}],
                "boards": []
            }}"#
        );
        (
            parse_index(index_raw.as_bytes()).unwrap(),
            parse_profile(&profile_raw, "pdf-viewer").unwrap(),
            sha256,
        )
    }

    #[test]
    fn cached_profile_resolves_immediately_without_a_worker_command() {
        let (index, profile, sha256) = index_and_profile();
        let expected_sha256 = sha256.clone();
        let profiles =
            HashMap::from([("pdf-viewer".to_string(), StoredProfile { sha256, profile })]);
        let (registry, receiver) = Registry::for_test(index, profiles);

        let preset = registry
            .lookup(&FocusedApplication::windows(7, "ACROBAT.EXE".into()))
            .unwrap();

        assert_eq!(preset.id, "pdf-viewer");
        assert_eq!(preset.name, "PDF 뷰어");
        assert_eq!(preset.sha256, expected_sha256);
        assert_eq!(preset.actions.len(), 1);
        assert_eq!(preset.actions[0].shortcut, "pagedown");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn missing_profile_queues_only_one_fetch_during_repeated_focus_polls() {
        let (index, _, _) = index_and_profile();
        let (registry, receiver) = Registry::for_test(index, HashMap::new());
        let app = FocusedApplication::windows(7, "Acrobat.exe".into());

        assert_eq!(registry.lookup(&app), None);
        assert_eq!(registry.lookup(&app), None);

        let Command::FetchProfile(entry) = receiver.try_recv().unwrap();
        assert_eq!(entry.id, "pdf-viewer");
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn unknown_application_does_not_queue_a_download() {
        let (index, _, _) = index_and_profile();
        let (registry, receiver) = Registry::for_test(index, HashMap::new());

        assert_eq!(
            registry.lookup(&FocusedApplication::windows(8, "Unknown.exe".into())),
            None
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn stale_last_known_good_profile_stays_active_while_its_update_is_queued() {
        let (mut index, profile, sha256) = index_and_profile();
        index.apps[0].sha256 = "0".repeat(64);
        let profiles =
            HashMap::from([("pdf-viewer".to_string(), StoredProfile { sha256, profile })]);
        let (registry, receiver) = Registry::for_test(index, profiles);

        let preset = registry
            .lookup(&FocusedApplication::windows(7, "Acrobat.exe".into()))
            .unwrap();

        assert_eq!(preset.id, "pdf-viewer");
        assert_eq!(preset.actions[0].shortcut, "pagedown");
        let Command::FetchProfile(entry) = receiver.try_recv().unwrap();
        assert_eq!(entry.sha256, "0".repeat(64));
    }
}
