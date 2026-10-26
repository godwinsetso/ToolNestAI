//! Voice catalogue, downloads and speech engines for the desktop app.
//!
//! The catalogue is the same file the Android app ships. Downloads resume,
//! are verified by SHA-256 before installation, and are only started by the
//! user. A voice can also be bundled with the installer (see
//! scripts/fetch-desktop-deps.sh) so READIT speaks offline from first launch.

use crate::player::SpeechEngine;
use parking_lot::Mutex;
use readit_core::model::InstalledVoice;
use readit_core::ReaditLibrary;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const CATALOG_JSON: &str = include_str!("../../../android/app/src/main/assets/voices.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInfo {
    pub id: String,
    pub speaker: i32,
    pub name: String,
    pub language: String,
    pub accent: String,
    pub gender: String,
    #[serde(default)]
    pub lexicon: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicePack {
    pub id: String,
    pub engine: String,
    pub title: String,
    pub archive: String,
    pub url: String,
    pub sha256: String,
    pub download_bytes: u64,
    pub installed_bytes: u64,
    pub licence: String,
    #[serde(default)]
    pub licence_url: String,
    pub quality: String,
    pub requirements: String,
    pub voices: Vec<VoiceInfo>,
}

#[derive(Deserialize)]
struct CatalogFile {
    packs: Vec<VoicePack>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadState {
    NotInstalled,
    Downloading { bytes: u64, total: u64 },
    Paused { bytes: u64, total: u64 },
    Verifying,
    Installing,
    Installed { bundled: bool },
    Failed { message: String, bytes: u64 },
}

pub struct VoiceManager {
    pub packs: Vec<VoicePack>,
    lib: Arc<ReaditLibrary>,
    user_dir: PathBuf,
    bundled_dir: Option<PathBuf>,
    partial_dir: PathBuf,
    states: Mutex<HashMap<String, DownloadState>>,
    pause_requests: Mutex<HashSet<String>>,
}

impl VoiceManager {
    pub fn new(lib: Arc<ReaditLibrary>, data_dir: &Path, bundled_dir: Option<PathBuf>) -> Self {
        let mut packs = serde_json::from_str::<CatalogFile>(CATALOG_JSON).expect("voices.json is valid").packs;
        // The shared catalogue speaks about phones; describe computers here.
        for p in &mut packs {
            p.requirements = if p.engine == "kokoro" {
                "Best on computers with 8 GB of memory and a processor from about 2018 or later; Test shows how it runs here.".into()
            } else {
                "Runs well on almost any computer.".into()
            };
        }
        let user_dir = data_dir.join("voices");
        let partial_dir = data_dir.join("voice-downloads");
        let _ = fs::create_dir_all(&user_dir);
        let _ = fs::create_dir_all(&partial_dir);
        let vm = Self {
            packs,
            lib,
            user_dir,
            bundled_dir,
            partial_dir,
            states: Mutex::new(HashMap::new()),
            pause_requests: Mutex::new(HashSet::new()),
        };
        // Register whatever is on disk (including a voice bundled with the installer).
        for p in vm.packs.clone() {
            if let Some(dir) = vm.installed_dir(&p) {
                let _ = vm.register(&p, &dir);
            }
        }
        vm
    }

    pub fn pack(&self, id: &str) -> Option<&VoicePack> {
        self.packs.iter().find(|p| p.id == id)
    }

    pub fn voice(&self, id: &str) -> Option<(&VoicePack, &VoiceInfo)> {
        self.packs.iter().find_map(|p| p.voices.iter().find(|v| v.id == id).map(|v| (p, v)))
    }

    fn is_complete(dir: &Path) -> bool {
        dir.join(".installed").exists() || dir.join("tokens.txt").exists() && dir.join("espeak-ng-data").is_dir()
    }

    /// Where an installed pack lives: the user's folder first, then the installer's bundle.
    pub fn installed_dir(&self, p: &VoicePack) -> Option<PathBuf> {
        let user = self.user_dir.join(&p.archive);
        if user.join(".installed").exists() {
            return Some(user);
        }
        let b = self.bundled_dir.as_ref()?.join(&p.archive);
        Self::is_complete(&b).then_some(b)
    }

    fn is_bundled(&self, p: &VoicePack) -> bool {
        self.installed_dir(p).map(|d| !d.starts_with(&self.user_dir)).unwrap_or(false)
    }

    fn partial(&self, p: &VoicePack) -> PathBuf {
        self.partial_dir.join(format!("{}.part", p.archive))
    }

    pub fn state(&self, id: &str) -> DownloadState {
        if let Some(s) = self.states.lock().get(id) {
            if !matches!(s, DownloadState::Installed { .. } | DownloadState::NotInstalled) {
                return s.clone();
            }
        }
        let Some(p) = self.pack(id) else { return DownloadState::NotInstalled };
        if self.installed_dir(p).is_some() {
            return DownloadState::Installed { bundled: self.is_bundled(p) };
        }
        match fs::metadata(self.partial(p)) {
            Ok(m) => DownloadState::Paused { bytes: m.len(), total: p.download_bytes },
            Err(_) => DownloadState::NotInstalled,
        }
    }

    fn set(&self, id: &str, s: DownloadState) {
        self.states.lock().insert(id.to_string(), s);
    }

    pub fn request_pause(&self, id: &str) {
        self.pause_requests.lock().insert(id.to_string());
    }

    pub fn free_bytes(&self) -> Option<u64> {
        free_space(&self.user_dir)
    }

    pub fn bytes_needed(&self, p: &VoicePack) -> u64 {
        let have = fs::metadata(self.partial(p)).map(|m| m.len()).unwrap_or(0);
        p.download_bytes.saturating_sub(have) + p.installed_bytes
    }

    /// Download (resuming), verify, extract and register. Blocking.
    /// `progress` is called with the new state as the download advances.
    pub fn install(&self, id: &str, progress: &dyn Fn(&DownloadState)) -> Result<(), String> {
        let p = self.pack(id).ok_or("Unknown voice pack")?.clone();
        self.pause_requests.lock().remove(id);
        let emit = |s: DownloadState| {
            self.set(id, s.clone());
            progress(&s);
        };
        if self.installed_dir(&p).is_some() {
            emit(DownloadState::Installed { bundled: self.is_bundled(&p) });
            return Ok(());
        }
        let part = self.partial(&p);
        let result = (|| -> Result<bool, String> {
            if let Some(free) = self.free_bytes() {
                let need = self.bytes_needed(&p) + 50_000_000;
                if free < need {
                    return Err(format!(
                        "Not enough free disk space. This voice needs about {} MB; {} MB is free.",
                        need / 1_000_000,
                        free / 1_000_000
                    ));
                }
            }
            if !self.download(&p, &part, &emit)? {
                return Ok(false); // paused
            }
            emit(DownloadState::Verifying);
            let hash = sha256_file(&part).map_err(|e| e.to_string())?;
            if !hash.eq_ignore_ascii_case(&p.sha256) {
                let _ = fs::remove_file(&part);
                return Err("The download was damaged (checksum mismatch) and has been deleted. Please try again.".into());
            }
            emit(DownloadState::Installing);
            extract_tbz(&part, &self.user_dir, &p.archive)?;
            let dir = self.user_dir.join(&p.archive);
            fs::write(dir.join(".installed"), &p.sha256).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(&part);
            self.register(&p, &dir)?;
            Ok(true)
        })();
        match result {
            Ok(true) => {
                emit(DownloadState::Installed { bundled: false });
                Ok(())
            }
            Ok(false) => {
                let bytes = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                emit(DownloadState::Paused { bytes, total: p.download_bytes });
                Ok(())
            }
            Err(e) => {
                let bytes = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                emit(DownloadState::Failed { message: e.clone(), bytes });
                Err(e)
            }
        }
    }

    /// Returns false if paused by the user.
    fn download(&self, p: &VoicePack, part: &Path, emit: &dyn Fn(DownloadState)) -> Result<bool, String> {
        let have = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if have >= p.download_bytes {
            return Ok(true);
        }
        let agent = ureq::AgentBuilder::new()
            .try_proxy_from_env(true)
            .timeout_connect(std::time::Duration::from_secs(20))
            .timeout_read(std::time::Duration::from_secs(60))
            .redirects(5)
            .build();
        let mut req = agent.get(&p.url);
        if have > 0 {
            req = req.set("Range", &format!("bytes={have}-"));
        }
        let resp = req.call().map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("The server replied {code}. Try again later."),
            ureq::Error::Transport(t) => {
                let detail = t.to_string();
                if detail.contains("certificate") {
                    "The secure connection to the download server could not be verified. On a work network, a firewall may be inspecting or blocking downloads; ask your IT team to allow github.com, or try another network.".to_string()
                } else if detail.contains("resolve") || detail.contains("dns") || detail.contains("Connection Failed") {
                    "Could not reach the download server. Check your internet connection and try again.".to_string()
                } else {
                    format!("The download could not start ({}). Try again.", detail.chars().take(120).collect::<String>())
                }
            }
        })?;
        if !resp.get_url().starts_with("https://") {
            return Err("Refusing an insecure download location.".into());
        }
        let append = resp.status() == 206;
        let mut f = OpenOptions::new().create(true).write(true).truncate(false).open(part).map_err(|e| e.to_string())?;
        let mut done = if append {
            f.seek(SeekFrom::Start(have)).map_err(|e| e.to_string())?;
            have
        } else {
            f.set_len(0).map_err(|e| e.to_string())?;
            0
        };
        let mut reader = resp.into_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut last = 0u64;
        loop {
            if self.pause_requests.lock().remove(&p.id) {
                return Ok(false);
            }
            let n = reader.read(&mut buf).map_err(|e| format!("The download was interrupted ({e}). Resume to continue."))?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if done - last > 512 * 1024 {
                emit(DownloadState::Downloading { bytes: done, total: p.download_bytes });
                last = done;
            }
        }
        Ok(true)
    }

    fn register(&self, p: &VoicePack, dir: &Path) -> Result<(), String> {
        let size = dir_size(dir);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        for v in &p.voices {
            self.lib
                .register_voice(InstalledVoice {
                    id: v.id.clone(),
                    engine: p.engine.clone(),
                    name: v.name.clone(),
                    language: v.language.clone(),
                    accent: v.accent.clone(),
                    gender: v.gender.clone(),
                    quality: p.quality.clone(),
                    licence: p.licence.clone(),
                    size_bytes: size,
                    offline: true,
                    path: dir.to_string_lossy().into_owned(),
                    installed_at: now,
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Removes a downloaded pack. A voice bundled with the installer cannot be removed here.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let p = self.pack(id).ok_or("Unknown voice pack")?.clone();
        if self.is_bundled(&p) {
            return Err("This voice came with READIT and is kept so you always have one offline voice.".into());
        }
        for v in &p.voices {
            let _ = self.lib.remove_voice(v.id.clone());
        }
        let _ = fs::remove_dir_all(self.user_dir.join(&p.archive));
        let _ = fs::remove_file(self.partial(&p));
        self.set(id, DownloadState::NotInstalled);
        Ok(())
    }

    /// The best installed voice: Kokoro Emma, then the Piper voices, then anything.
    pub fn default_voice(&self) -> Option<String> {
        let installed: Vec<String> = self.lib.installed_voices().ok()?.into_iter().map(|v| v.id).collect();
        ["kokoro:bf_emma", "piper:cori", "piper:alba", "piper:northern"]
            .iter()
            .find(|v| installed.iter().any(|i| i == *v))
            .map(|s| s.to_string())
            .or_else(|| installed.first().cloned())
    }

    pub fn create_engine(&self, voice_id: &str) -> Result<Box<dyn SpeechEngine>, String> {
        let (p, v) = self.voice(voice_id).ok_or("Unknown voice")?;
        let dir = self.installed_dir(p).ok_or_else(|| format!("{} is not downloaded yet.", v.name))?;
        SherpaEngine::new(&dir, p, v).map(|e| Box::new(e) as Box<dyn SpeechEngine>)
    }

    /// Real-time factor on this computer: below 1.0 renders faster than it plays.
    pub fn benchmark(&self, voice_id: &str) -> Result<f64, String> {
        let mut e = self.create_engine(voice_id)?;
        e.synthesize("Warming up.", 1.0)?;
        let t = std::time::Instant::now();
        let audio = e.synthesize("Financial institutions must verify the identity of every customer before opening an account.", 1.0)?;
        let rtf = t.elapsed().as_secs_f64() / (audio.len() as f64 / e.sample_rate() as f64).max(0.1);
        let _ = self.lib.set_setting(format!("rtf:{voice_id}"), format!("{rtf:.3}"));
        Ok(rtf)
    }

    pub fn measured_rtf(&self, voice_id: &str) -> Option<f64> {
        self.lib.setting(format!("rtf:{voice_id}")).ok().flatten().and_then(|s| s.parse().ok())
    }
}

// ------------------------------------------------------------------ engine

enum Inner {
    Kokoro(sherpa_rs::tts::KokoroTts),
    Vits(sherpa_rs::tts::VitsTts),
}

pub struct SherpaEngine {
    id: String,
    sid: i32,
    rate: u32,
    tts: Inner,
}

fn path_str(p: PathBuf) -> Result<String, String> {
    if !p.exists() {
        return Err(format!("The voice files are incomplete ({} is missing). Remove the voice and download it again.", p.display()));
    }
    Ok(p.to_string_lossy().into_owned())
}

impl SherpaEngine {
    pub fn new(dir: &Path, p: &VoicePack, v: &VoiceInfo) -> Result<Self, String> {
        let threads = std::thread::available_parallelism().map(|n| n.get().clamp(1, 4)).unwrap_or(2) as i32;
        let onnx = sherpa_rs::OnnxConfig { provider: "cpu".into(), debug: false, num_threads: threads };
        let data_dir = path_str(dir.join("espeak-ng-data"))?;
        let tokens = path_str(dir.join("tokens.txt"))?;
        let tts = if p.engine == "kokoro" {
            let lexicon = match &v.lexicon {
                Some(l) => path_str(dir.join(l))?,
                None => String::new(),
            };
            Inner::Kokoro(sherpa_rs::tts::KokoroTts::new(sherpa_rs::tts::KokoroTtsConfig {
                model: path_str(dir.join("model.int8.onnx"))?,
                voices: path_str(dir.join("voices.bin"))?,
                tokens,
                data_dir,
                lexicon,
                // Multi-lingual Kokoro (v1.0+) requires its dict folder; without it the engine exits the process.
                dict_dir: if dir.join("dict").is_dir() { path_str(dir.join("dict"))? } else { String::new() },
                length_scale: 1.0,
                onnx_config: onnx,
                ..Default::default()
            }))
        } else {
            let model = v.model.as_ref().ok_or("Piper voice without a model file")?;
            Inner::Vits(sherpa_rs::tts::VitsTts::new(sherpa_rs::tts::VitsTtsConfig {
                model: path_str(dir.join(model))?,
                tokens,
                data_dir,
                length_scale: 1.0,
                noise_scale: 0.667,
                noise_scale_w: 0.8,
                onnx_config: onnx,
                ..Default::default()
            }))
        };
        let mut e = Self { id: v.id.clone(), sid: v.speaker, rate: 0, tts };
        // Learn the sample rate from a tiny render (the C API reports it per result).
        let probe = e.raw(".", 1.0)?;
        e.rate = probe.1;
        Ok(e)
    }

    fn raw(&mut self, text: &str, speed: f32) -> Result<(Vec<f32>, u32), String> {
        let r = match &mut self.tts {
            Inner::Kokoro(t) => t.create(text, self.sid, speed),
            Inner::Vits(t) => t.create(text, self.sid, speed),
        }
        .map_err(|e| e.to_string())?;
        Ok((r.samples, r.sample_rate))
    }
}

impl SpeechEngine for SherpaEngine {
    fn voice_id(&self) -> &str {
        &self.id
    }
    fn sample_rate(&self) -> u32 {
        self.rate
    }
    fn synthesize(&mut self, text: &str, speed: f32) -> Result<Vec<f32>, String> {
        if text.trim().is_empty() {
            return Ok(vec![]);
        }
        let (samples, _) = self.raw(text, speed)?;
        if samples.is_empty() {
            return Err("the voice produced no audio for this sentence".into());
        }
        Ok(samples)
    }
}

// ------------------------------------------------------------------ helpers

fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Extract a .tar.bz2 into `into/<top>`, refusing links and any path that escapes the folder.
pub fn extract_tbz(archive: &Path, into: &Path, top: &str) -> Result<(), String> {
    let staging = into.join(format!(".staging-{top}"));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let f = File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::BufReader::new(f)));
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut e = entry.map_err(|e| format!("The voice archive is damaged: {e}"))?;
        let path = e.path().map_err(|e| e.to_string())?.into_owned();
        if path.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
            return Err(format!("Unsafe path in voice archive: {}", path.display()));
        }
        let kind = e.header().entry_type();
        if kind.is_symlink() || kind.is_hard_link() {
            continue;
        }
        let out = staging.join(&path);
        if kind.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else if kind.is_file() {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut w = File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut e, &mut w).map_err(|e| e.to_string())?;
        }
    }
    let extracted = staging.join(top);
    if !extracted.is_dir() {
        let _ = fs::remove_dir_all(&staging);
        return Err("The voice archive has an unexpected layout.".into());
    }
    let target = into.join(top);
    let _ = fs::remove_dir_all(&target);
    fs::rename(&extracted, &target).map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&staging);
    Ok(())
}

fn dir_size(p: &Path) -> u64 {
    fs::read_dir(p)
        .map(|rd| {
            rd.flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => dir_size(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(unix)]
fn free_space(p: &Path) -> Option<u64> {
    let c = std::ffi::CString::new(p.to_string_lossy().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    let r = unsafe { libc::statvfs(c.as_ptr(), &mut s) };
    (r == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
}

#[cfg(windows)]
fn free_space(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free: u64 = 0;
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_has_offline_fallback() {
        let c: CatalogFile = serde_json::from_str(CATALOG_JSON).unwrap();
        assert!(c.packs.iter().any(|p| p.id == "piper-cori"));
        assert!(c.packs.iter().all(|p| p.sha256.len() == 64));
    }

    #[test]
    fn extraction_refuses_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let arc = dir.path().join("evil.tar.bz2");
        {
            let f = File::create(&arc).unwrap();
            let mut b = tar::Builder::new(bzip2::write::BzEncoder::new(f, bzip2::Compression::fast()));
            let data = b"x";
            let mut h = tar::Header::new_gnu();
            h.set_size(1);
            h.set_mode(0o644);
            // Write the raw name so the builder does not reject it.
            h.as_gnu_mut().unwrap().name[..12].copy_from_slice(b"../escape.sh");
            h.set_cksum();
            b.append(&h, &data[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let err = extract_tbz(&arc, dir.path(), "pack").unwrap_err();
        assert!(err.contains("Unsafe path"), "{err}");
        assert!(!dir.path().parent().unwrap().join("escape.sh").exists());
    }
}
