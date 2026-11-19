//! Reading a track from what the user typed or pasted.

use anyhow::{Context, Result};
use librespot::core::spotify_id::SpotifyId;

/// Accept a bare track ID, `spotify:track:<id>`, or `https://open.spotify.com/track/<id>?si=...`.
pub fn parse_track(input: &str) -> Result<SpotifyId> {
    let input = input.trim();
    // Drop the "?si=..." share tracking part first, so the "/track/" match only looks at the path.
    let path = input.split(['?', '#']).next().unwrap_or(input);
    let id = if let Some(rest) = input.strip_prefix("spotify:track:") {
        rest
    } else if let Some((_, rest)) = path.split_once("open.spotify.com/track/") {
        rest.trim_end_matches('/')
    } else {
        input
    };
    SpotifyId::from_base62(id).with_context(|| format!("not a Spotify track ID or link: {input}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "4uLU6hMCjMI75M1A2tKUQC";

    #[test]
    fn parse_track_accepts_id_uri_and_link() {
        let expected = SpotifyId::from_base62(ID).unwrap();
        assert_eq!(parse_track(ID).unwrap(), expected);
        assert_eq!(
            parse_track(&format!("spotify:track:{ID}")).unwrap(),
            expected
        );
        assert_eq!(
            parse_track(&format!("https://open.spotify.com/track/{ID}?si=abc123")).unwrap(),
            expected
        );
        assert_eq!(
            parse_track(&format!("  https://open.spotify.com/track/{ID}/  ")).unwrap(),
            expected
        );
    }

    #[test]
    fn parse_track_rejects_garbage() {
        assert!(parse_track("").is_err());
        assert!(parse_track("not an id!").is_err());
        assert!(parse_track("https://open.spotify.com/track/").is_err());
        // "/track/" hidden in the query part is not a track link.
        assert!(
            parse_track(&format!(
                "https://open.spotify.com/?si=open.spotify.com/track/{ID}"
            ))
            .is_err()
        );
    }
}
