//! Read-only check of YouTube Music's library using the cached session.
use anyhow::{Context as _, Result};
use music::youtube::YouTubeProvider;
use music::MusicProvider;

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let provider = YouTubeProvider::new();
    let session = provider
        .restore()
        .await?
        .context("no cached YouTube Music session")?;
    let items = session
        .api
        .pin_targets()
        .await?
        .context("provider keeps no pins")?;
    println!(
        "library items={}, pinned={}",
        items.len(),
        items.iter().filter(|item| item.pinned).count()
    );
    for item in items.iter().take(10) {
        println!("{:?} pinned={} {}", item.kind, item.pinned, item.name);
    }
    Ok(())
}