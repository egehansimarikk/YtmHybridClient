use anyhow::{Context as _, Result, bail};

use crate::youtube::YouTubeProvider;
use crate::{MusicApi, MusicProvider, ReleaseType};

/// A release every catalogue files as an EP, and the query that finds it.
const ARTIST: &str = "Sylosis";
const TITLE: &str = "The Path";

#[tokio::test]
#[ignore = "reads the catalogue through the stored YouTube Music session"]
async fn youtube_labels_an_ep_as_one() -> Result<()> {
    album_is_an_ep(&YouTubeProvider::new()).await
}

async fn album_is_an_ep(provider: &dyn MusicProvider) -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let session = provider
        .restore()
        .await?
        .with_context(|| format!("{} has no stored Sonora session", provider.name()))?;
    ep_is_labelled(provider.name(), session.api.as_ref()).await
}

/// Finds the EP by search and checks both the search hit and the album page call it an EP.
async fn ep_is_labelled(provider: &str, api: &dyn MusicApi) -> Result<()> {
    let hits = api
        .search_albums(&format!("{ARTIST} {TITLE}"))
        .await
        .with_context(|| format!("{provider} could not search albums"))?;
    let Some(hit) = hits
        .iter()
        .find(|album| album.name.starts_with(TITLE) && album.artists.contains(ARTIST))
    else {
        bail!(
            "{provider} found no {ARTIST} - {TITLE} among {} hits",
            hits.len()
        );
    };
    let detail = api
        .album(&hit.id)
        .await
        .with_context(|| format!("{provider} could not load album {}", hit.id))?;

    let found = (hit.release_type, detail.album.release_type);
    if found != (ReleaseType::Ep, ReleaseType::Ep) {
        bail!("{provider} labels {TITLE} as {found:?} (search, page), not an EP");
    }
    Ok(())
}