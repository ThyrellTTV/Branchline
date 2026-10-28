//! Author avatars, fetched in the background, cached on disk, painted round. A repository on
//! GitHub or GitLab tells us (through [`hint`]) where its authors' account pictures are; other
//! addresses go to Gravatar (or GitHub for its noreply ones). Initials stand in until (or unless)
//! one loads.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use egui::{Rect, Vec2, load::Bytes};

/// A missing avatar is asked for again after this long.
const RETRY_MISSING: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone)]
enum State {
    Pending,
    /// The picture, and its cache key (which says where it came from).
    Ready(Arc<[u8]>, String),
    Missing,
}

struct Avatars {
    map: Mutex<HashMap<String, State>>,
    queue: Mutex<Sender<String>>,
}

/// Where an address's picture is, when a forge knows better than Gravatar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hint {
    /// The account's avatar URL (GitHub).
    Url(String),
    /// Ask this GitLab host's avatar API.
    GitLab(String),
}

static AVATARS: OnceLock<Avatars> = OnceLock::new();

fn hints() -> &'static Mutex<HashMap<String, Hint>> {
    static HINTS: OnceLock<Mutex<HashMap<String, Hint>>> = OnceLock::new();
    HINTS.get_or_init(Default::default)
}

/// Records where `email`'s picture is; one already shown from elsewhere is fetched again.
pub fn hint(email: &str, hint: Hint) {
    let k = email.trim().to_lowercase();
    let mut h = lock(hints());
    if h.get(&k) == Some(&hint) {
        return;
    }
    h.insert(k.clone(), hint);
    drop(h);
    if let Some(av) = AVATARS.get() {
        lock(&av.map).remove(&k);
    }
}

fn cache_dir() -> Option<PathBuf> {
    eframe::storage_dir("gitcraft").map(|d| d.join("avatars"))
}

fn avatars(ctx: &egui::Context) -> &'static Avatars {
    AVATARS.get_or_init(|| {
        let (tx, rx) = channel::<String>();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(8))
                .user_agent("gitcraft")
                .build();
            while let Ok(email) = rx.recv() {
                let hint = lock(hints()).get(&email).cloned();
                let state = fetch(&agent, &email, hint.as_ref());
                if let Some(map) = AVATARS.get() {
                    lock(&map.map).insert(email, state);
                }
                ctx.request_repaint();
            }
        });
        Avatars {
            map: Mutex::new(HashMap::new()),
            queue: Mutex::new(tx),
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

fn key(email: &str) -> String {
    format!("{:x}", md5::compute(email.trim().to_lowercase()))
}

/// Where to ask: GitHub knows its noreply addresses, Gravatar the rest.
fn url(email: &str) -> String {
    let email = email.trim().to_lowercase();
    if let Some(local) = email.strip_suffix("@users.noreply.github.com") {
        return match local.split_once('+') {
            Some((id, _)) => format!("https://avatars.githubusercontent.com/u/{id}?s=80"),
            None => format!("https://github.com/{local}.png?size=80"),
        };
    }
    format!("https://www.gravatar.com/avatar/{}?s=80&d=404", key(&email))
}

/// Cache file name: Gravatar's hash, plus where the picture came from when a forge said.
fn cache_key(email: &str, hint: Option<&Hint>) -> String {
    match hint {
        None => key(email),
        Some(Hint::Url(_)) => format!("{}-gh", key(email)),
        Some(Hint::GitLab(host)) => format!("{}-gl-{}", key(email), key(host)),
    }
}

/// The picture's URL: a forge's, else [`url`].
fn source(agent: &ureq::Agent, email: &str, hint: Option<&Hint>) -> String {
    match hint {
        Some(Hint::Url(u)) => format!("{u}{}s=80", if u.contains('?') { '&' } else { '?' }),
        Some(Hint::GitLab(host)) => {
            let api = format!(
                "https://{host}/api/v4/avatar?email={}&size=80",
                query_escape(email)
            );
            let found = agent
                .get(&api)
                .call()
                .ok()
                .and_then(|r| r.into_string().ok())
                .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
                .and_then(|v| v.get("avatar_url")?.as_str().map(str::to_owned));
            match found {
                // GitLab falls back to a generated Gravatar; initials read better.
                Some(u) if !u.contains("gravatar.com") => u,
                _ => url(email),
            }
        }
        None => url(email),
    }
}

fn query_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' | b'@' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn fetch(agent: &ureq::Agent, email: &str, hint: Option<&Hint>) -> State {
    let dir = cache_dir();
    let k = cache_key(email, hint);
    if let Some(dir) = &dir {
        if let Ok(bytes) = std::fs::read(dir.join(format!("{k}.img"))) {
            return State::Ready(Arc::from(bytes.into_boxed_slice()), k);
        }
        let missing = dir.join(format!("{k}.none"));
        let recent = std::fs::metadata(&missing)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < RETRY_MISSING);
        if recent {
            return State::Missing;
        }
    }
    let result = agent
        .get(&source(agent, email, hint))
        .call()
        .ok()
        .and_then(|resp| {
            let mut bytes = Vec::new();
            use std::io::Read;
            resp.into_reader()
                .take(512 * 1024)
                .read_to_end(&mut bytes)
                .ok()
                .map(|_| bytes)
        });
    match result {
        Some(bytes) if !bytes.is_empty() => {
            if let Some(dir) = &dir {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{k}.img")), &bytes);
            }
            State::Ready(Arc::from(bytes.into_boxed_slice()), k)
        }
        _ => {
            if let Some(dir) = &dir {
                let _ = std::fs::create_dir_all(dir);
                let _ = std::fs::write(dir.join(format!("{k}.none")), b"");
            }
            State::Missing
        }
    }
}

/// Paints `email`'s avatar into `rect` if it is loaded; returns false (and queues a download)
/// otherwise, so the caller keeps its initials.
pub fn paint(ui: &egui::Ui, rect: Rect, email: &str) -> bool {
    if email.is_empty() || !crate::settings::Settings::get(ui.ctx()).avatars {
        return false;
    }
    let av = avatars(ui.ctx());
    let k = email.trim().to_lowercase();
    let state = {
        let mut map = lock(&av.map);
        match map.get(&k) {
            Some(s) => s.clone(),
            None => {
                map.insert(k.clone(), State::Pending);
                let _ = lock(&av.queue).send(k.clone());
                State::Pending
            }
        }
    };
    let State::Ready(bytes, source) = state else {
        return false;
    };
    // egui keeps an image per URI: one per source, so a forge's picture replaces Gravatar's.
    let uri = format!("bytes://avatar/{source}.png");
    egui::Image::from_bytes(uri, Bytes::Shared(bytes))
        .fit_to_exact_size(Vec2::splat(rect.width()))
        .corner_radius(rect.width() / 2.0)
        .show_loading_spinner(false)
        .paint_at(ui, rect);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hinted_sources() {
        let agent = ureq::AgentBuilder::new().build();
        assert_eq!(
            source(
                &agent,
                "a@b.c",
                Some(&Hint::Url("https://av/u/1?v=4".into()))
            ),
            "https://av/u/1?v=4&s=80"
        );
        assert_eq!(query_escape("me+git@x.org"), "me%2Bgit@x.org");
        assert_ne!(
            cache_key("a@b.c", None),
            cache_key("a@b.c", Some(&Hint::Url(String::new())))
        );
    }

    #[test]
    fn avatar_urls() {
        assert_eq!(
            url("12345+octo@users.noreply.github.com"),
            "https://avatars.githubusercontent.com/u/12345?s=80"
        );
        assert_eq!(
            url("octo@users.noreply.github.com"),
            "https://github.com/octo.png?size=80"
        );
        // Gravatar hashes the trimmed, lowercased address.
        assert_eq!(
            url(" MyEmailAddress@example.com "),
            "https://www.gravatar.com/avatar/0bc83cb571cd1c50ba6f3e8a78ef1346?s=80&d=404"
        );
    }
}
