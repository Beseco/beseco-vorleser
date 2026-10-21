//! Ollama auf diesem PC: erkennen, starten, installieren und Modelle herunterladen. Für den
//! Einrichtungsassistenten des Officeassistenten.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::aicheck::Hardware;
use crate::http::{self, Request};
use crate::i18n::t;
use crate::json::{self, Value};

/// Adresse von Ollama auf diesem PC.
const LOCAL_URL: &str = "http://localhost:11434";
/// Immer die neueste Version, direkt von GitHub (geht auch über den Firmen-Proxy).
const SETUP_URL: &str = "https://github.com/ollama/ollama/releases/latest/download/OllamaSetup.exe";
/// Ungefähre Größe des Installationsprogramms in MB, für den Hinweis vorab.
pub const SETUP_MB: u64 = 1700;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Ein Modell, das der Assistent für diesen PC vorschlägt.
pub struct Suggestion {
    pub name: &'static str,
    /// Download in MB.
    pub size_mb: u64,
}

/// Adresse von Ollama auf diesem PC (im Testmodus umlenkbar).
pub fn local_url() -> String {
    std::env::var_os("VORLESER_TEST")
        .and(std::env::var("VORLESER_OLLAMA_URL").ok())
        .unwrap_or_else(|| LOCAL_URL.to_string())
}

/// Antwortet unter `url` ein Ollama?
pub fn is_running(url: &str) -> bool {
    let address = format!("{}/api/version", url.trim_end_matches('/'));
    let mut request = Request::get(&address);
    request.timeout = 3000;
    matches!(http::fetch(&request), Ok((200, body)) if body.contains("version"))
}

/// Das Programm, das Ollama im Infobereich startet (es startet den Server mit).
fn app_exe() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    let candidates = [
        local.join("Programs").join("Ollama").join("ollama app.exe"),
        std::env::var_os("ProgramFiles").map(PathBuf::from)?.join("Ollama").join("ollama app.exe"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

pub fn is_installed() -> bool {
    app_exe().is_some()
}

/// Startet das installierte Ollama und wartet, bis es antwortet.
pub fn start(url: &str) -> Result<(), String> {
    if is_running(url) {
        return Ok(());
    }
    let exe = app_exe().ok_or_else(|| t("setup.err_not_installed").to_string())?;
    Command::new(exe).creation_flags(CREATE_NO_WINDOW).spawn().map_err(|e| e.to_string())?;
    wait_until_running(url, Duration::from_secs(90))
}

fn wait_until_running(url: &str, limit: Duration) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < limit {
        if is_running(url) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    Err(t("setup.err_no_start").to_string())
}

/// Lädt das Installationsprogramm von Ollama, installiert still für dieses Benutzerkonto
/// (ohne Administratorrechte) und startet Ollama.
pub fn install(url: &str, progress: impl FnMut(u64, u64) -> bool) -> Result<(), String> {
    let setup = std::env::temp_dir().join("OllamaSetup.exe");
    http::download(SETUP_URL, &setup, progress)?;
    let status = Command::new(&setup)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"])
        .status()
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&setup);
    if !status?.success() {
        return Err(t("setup.err_install").to_string());
    }
    // Das Installationsprogramm startet Ollama meist selbst.
    if wait_until_running(url, Duration::from_secs(20)).is_ok() {
        return Ok(());
    }
    start(url)
}

/// Lädt ein Modell in Ollama; `progress` erhält geladene und gesamte Bytes und bricht mit
/// `false` ab.
pub fn pull(url: &str, model: &str, mut progress: impl FnMut(u64, u64) -> bool) -> Result<(), String> {
    let address = format!("{}/api/pull", url.trim_end_matches('/'));
    let body = format!("{{\"model\":{},\"stream\":true}}", json::string(model));
    let request = Request {
        method: "POST",
        url: &address,
        headers: vec!["Content-Type: application/json".to_string()],
        body: Some(body.as_bytes()),
        timeout: 600_000,
    };
    let mut error = None;
    let mut success = false;
    let mut cancelled = false;
    let status = http::stream(&request, |line| {
        let Some(value) = json::parse(line) else { return true };
        if let Some(message) = value.get("error").and_then(Value::as_str) {
            error = Some(message.to_string());
            return false;
        }
        if value.get("status").and_then(Value::as_str) == Some("success") {
            success = true;
        }
        let number = |key| match value.get(key) {
            Some(Value::Number(n)) => *n as u64,
            _ => 0,
        };
        let total = number("total");
        cancelled = total > 0 && !progress(number("completed"), total);
        !cancelled
    });
    if cancelled {
        return Err(t("err.cancelled").to_string());
    }
    if let Some(error) = error {
        return Err(format!("{} {error}", t("setup.err_pull")));
    }
    match status {
        Ok(200) if success => Ok(()),
        Ok(status) => Err(format!("{} (HTTP {status})", t("setup.err_pull"))),
        Err(error) => Err(error),
    }
}

/// Modell passend zum Rechner: mit Grafikkarte größer und besser, sonst klein und flott genug.
pub fn suggest(hardware: &Hardware) -> Suggestion {
    let vram = hardware.gpu.as_ref().map_or(0.0, |(_, vram)| *vram);
    if vram >= 10.0 && hardware.ram_gb >= 15.0 {
        Suggestion { name: "gemma3:12b", size_mb: 8100 }
    } else {
        // Gutes Deutsch, und ohne Nachdenken (siehe `ai::Provider::chat`) auch auf dem
        // Prozessor flott genug.
        Suggestion { name: "qwen3.5:4b", size_mb: 3300 }
    }
}

/// Wählt aus den Modellen eines Anbieters ein gut geeignetes Sprachmodell (Bild-, Embedding- und
/// Sprachmodelle fallen weg). `None`, wenn kein Sprachmodell dabei ist.
pub fn choose_model(models: &[String]) -> Option<String> {
    const SKIP: [&str; 9] = ["embed", "bge", "flux", "stable-diffusion", "whisper", "tts", "rerank", "vision-only", "guard"];
    const PREFERRED: [&str; 10] =
        ["llama-3.3", "llama3.3", "gpt-oss", "mistral-small", "gemma3", "qwen2.5", "qwen3", "llama3.1", "llama-3.1", "mistral"];
    let usable: Vec<&String> = models.iter().filter(|m| !SKIP.iter().any(|s| m.to_lowercase().contains(s))).collect();
    PREFERRED
        .iter()
        .find_map(|p| usable.iter().find(|m| m.to_lowercase().contains(p)))
        .or(usable.first())
        .map(|m| m.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn chooses_language_models() {
        let ionos = list(&["BAAI/bge-m3", "black-forest-labs/FLUX.1-schnell", "meta-llama/Llama-3.3-70B-Instruct", "mistralai/Mistral-Nemo"]);
        assert_eq!(choose_model(&ionos).as_deref(), Some("meta-llama/Llama-3.3-70B-Instruct"));
        let ollama = list(&["nomic-embed-text:latest", "phi4:latest", "qwen2.5:7b"]);
        assert_eq!(choose_model(&ollama).as_deref(), Some("qwen2.5:7b"));
        assert_eq!(choose_model(&list(&["phi4:latest"])).as_deref(), Some("phi4:latest"));
        assert_eq!(choose_model(&list(&["nomic-embed-text"])), None);
        assert_eq!(choose_model(&[]), None);
    }

    /// Echtes Ollama auf diesem PC: lädt ein winziges Modell, fragt es und löscht es wieder.
    /// `cargo test real_ollama -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_ollama() {
        use crate::ai::{Kind, Provider};
        let url = LOCAL_URL;
        assert!(is_running(url), "Ollama läuft nicht");
        let model = "smollm2:135m";
        let mut steps = 0;
        let start = Instant::now();
        pull(url, model, |done, total| {
            steps += 1;
            if steps % 200 == 0 {
                println!("{} von {} MB", done / 1_000_000, total / 1_000_000);
            }
            true
        })
        .unwrap();
        println!("geladen in {:?} ({steps} Meldungen)", start.elapsed());
        let provider = Provider { kind: Kind::Ollama, url: url.to_string(), key: String::new(), model: String::new() };
        let models = provider.models().unwrap();
        println!("Modelle: {models:?}, gewählt: {:?}", choose_model(&models));
        assert!(models.iter().any(|m| m.starts_with("smollm2")));
        let provider = Provider { model: model.to_string(), ..provider };
        let (ms, answer) = provider.test().unwrap();
        println!("Antwort nach {ms} ms: {answer}");
        let body = format!("{{\"model\":{}}}", json::string(model));
        let address = format!("{url}/api/delete");
        let request = Request { method: "DELETE", url: &address, headers: vec!["Content-Type: application/json".into()], body: Some(body.as_bytes()), timeout: 30_000 };
        println!("gelöscht: {:?}", http::fetch(&request).map(|(s, _)| s));
    }

    #[test]
    fn suggests_by_hardware() {
        let pc = |gpu: Option<f64>, ram: f64| Hardware {
            cpu: String::new(),
            threads: 8,
            avx2: true,
            ram_gb: ram,
            gpu: gpu.map(|v| ("GPU".to_string(), v)),
        };
        assert_eq!(suggest(&pc(None, 16.0)).name, "qwen3.5:4b");
        assert_eq!(suggest(&pc(Some(8.0), 32.0)).name, "qwen3.5:4b");
        assert_eq!(suggest(&pc(Some(12.0), 32.0)).name, "gemma3:12b");
    }
}
