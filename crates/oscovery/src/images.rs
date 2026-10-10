//! Preview images: fetched once into `~/.cache/oscovery/images/`, decoded
//! off the main thread (the marketplace serves WebP, which GTK's own loaders
//! don't read and a pixbuf loader for may not be installed), and shown as a
//! texture. A picture that can't be loaded stays on its fallback; nothing is
//! retried within a run.

use crate::rt;
use gtk::{gdk, glib};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// Larger images are scaled down before they become textures.
const MAX_EDGE: u32 = 1600;

thread_local! {
    /// URLs that failed this run, so a card scrolled back into view doesn't
    /// hit the network again.
    static FAILED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(oscovery_core::index::USER_AGENT)
            .timeout(Duration::from_secs(30))
            .build()
            .expect("a reqwest client")
    })
}

fn cache_path(url: &str) -> PathBuf {
    glib::user_cache_dir()
        .join(crate::config::DIR_NAME)
        .join("images")
        .join(hex::encode(Sha256::digest(url.as_bytes())))
}

struct Decoded {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Show `url` in `picture` once it is here; `on_shown` runs then, for the
/// caller to swap a placeholder out.
pub fn load_into(picture: &gtk::Picture, url: &str, on_shown: impl Fn() + 'static) {
    if url.is_empty() || FAILED.with(|failed| failed.borrow().contains(url)) {
        return;
    }
    let url = url.to_string();
    let job_url = url.clone();
    let path = cache_path(&url);
    rt::run(
        async move { load(&job_url, &path).await },
        glib::clone!(
            #[weak]
            picture,
            move |result| match result {
                Ok(decoded) => {
                    let stride = decoded.width as usize * 4;
                    let texture = gdk::MemoryTexture::new(
                        decoded.width as i32,
                        decoded.height as i32,
                        gdk::MemoryFormat::R8g8b8a8,
                        &glib::Bytes::from_owned(decoded.rgba),
                        stride,
                    );
                    picture.set_paintable(Some(&texture));
                    on_shown();
                }
                Err(error) => {
                    tracing::debug!("image {url}: {error}");
                    FAILED.with(|failed| failed.borrow_mut().insert(url.clone()));
                }
            }
        ),
    );
}

async fn load(url: &str, path: &Path) -> Result<Decoded, String> {
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => fetch(url, path).await?,
    };
    tokio::task::spawn_blocking(move || decode(&bytes))
        .await
        .map_err(|e| e.to_string())?
}

async fn fetch(url: &str, target: &Path) -> Result<Vec<u8>, String> {
    let response = client()
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if let Some(dir) = target.parent() {
        tokio::fs::create_dir_all(dir)
            .await
            .map_err(|e| e.to_string())?;
    }
    // Write beside, then rename, so a half-written file is never read.
    let partial = target.with_extension("part");
    tokio::fs::write(&partial, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&partial, target)
        .await
        .map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

fn decode(bytes: &[u8]) -> Result<Decoded, String> {
    let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let image = if image.width().max(image.height()) > MAX_EDGE {
        image.thumbnail(MAX_EDGE, MAX_EDGE)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}
