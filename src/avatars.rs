//! Author avatars from Gravatar (or GitHub for its noreply addresses), fetched in the
//! background, cached on disk, painted round. Initials stand in until (or unless) one loads.

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
    Ready(Arc<[u8]>),
    Missing,
}

struct Avatars {
    map: Mutex<HashMap<String, State>>,
    queue: Mutex<Sender<String>>,
}

fn cache_dir() -> Option<PathBuf> {
    eframe::storage_dir("gitcraft").map(|d| d.join("avatars"))
}

fn avatars(ctx: &egui::Context) -> &'static Avatars {
    static AVATARS: OnceLock<Avatars> = OnceLock::new();
    AVATARS.get_or_init(|| {
        let (tx, rx) = channel::<String>();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let agent = ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(8))
                .user_agent("gitcraft")
                .build();
            while let Ok(email) = rx.recv() {
                let state = fetch(&agent, &email);
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

fn fetch(agent: &ureq::Agent, email: &str) -> State {
    let dir = cache_dir();
    let k = key(email);
    if let Some(dir) = &dir {
        if let Ok(bytes) = std::fs::read(dir.join(format!("{k}.img"))) {
            return State::Ready(Arc::from(bytes.into_boxed_slice()));
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
    let result = agent.get(&url(email)).call().ok().and_then(|resp| {
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
            State::Ready(Arc::from(bytes.into_boxed_slice()))
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
    let State::Ready(bytes) = state else {
        return false;
    };
    let uri = format!("bytes://avatar/{}.png", key(&k));
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
