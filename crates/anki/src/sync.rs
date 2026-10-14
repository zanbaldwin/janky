//! Syncs a copy of a user's collection with an Anki sync server.
//!
//! janky keeps its own copy of the collection and syncs it like one more device. The copy holds
//! nothing that the server or the project does not have, so [`Session::pull`] answers a
//! full-sync request with a full download. Only [`Session::full_upload`] can replace the
//! server's collection, and the caller decides when to call it.
//!
//! [`Session::media_names`] and [`Session::download_media`] get single media files from the
//! server without a collection, for `janky pull`.
//!
//! rslib's sync code is async. A [`Session`] owns a Tokio runtime with one worker thread, like
//! rslib's own backend, and blocks on each call.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::path::Path;

use anki::collection::{Collection, CollectionBuilder};
use anki::error::{AnkiError, SyncErrorKind};
use anki::prelude::{I18n, Usn};
use anki::sync::collection::normal::SyncActionRequired;
use anki::sync::http_client::HttpSyncClient;
use anki::sync::login::{SyncAuth, sync_login};
use anki::sync::media::begin::SyncBeginRequest;
use anki::sync::media::changes::MediaChangesRequest;
use anki::sync::media::download::DownloadFilesRequest;
use anki::sync::media::protocol::MediaSyncProtocol;
use anki::sync::request::IntoSyncRequest;
use reqwest::{Client, Url};
use tokio::runtime::Runtime;

use crate::{Error, Result, fail};

/// Opens the copy at `path`, or makes an empty one there, with the desktop media layout: the
/// folder `<stem>.media/` and the database `<stem>.mdb` next to the file.
pub fn open_copy(path: &Path) -> Result<Collection> {
    CollectionBuilder::new(path).with_desktop_media_paths().build().map_err(fail)
}

/// What [`Session::pull`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pulled {
    /// A normal sync, in both directions.
    Normal,
    /// The server asked for a full sync, so the copy was replaced with the server's collection.
    FullDownload,
}

/// What the server answered to [`Session::push`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pushed {
    /// The normal sync sent every change.
    Done,
    /// The schema changed, or the server's collection has no cards yet. Only a full upload or a
    /// full download can continue. Nothing was sent.
    FullSyncRequired {
        /// The server's collection has no cards, so a full upload loses nothing there.
        server_empty: bool,
    },
}

/// A connection to a sync server after a successful login.
///
/// It has no `Debug` output, because it holds the host key.
pub struct Session {
    auth: SyncAuth,
    client: Client,
    runtime: Runtime,
}

impl Session {
    /// Logs in to the sync server at `endpoint` and keeps the host key in memory only.
    ///
    /// `endpoint` must be a checked address that ends with `/`. A refused user name or password
    /// gives an [`Error`] that says so.
    ///
    /// The client follows no redirects, because rslib sends the host key in a header that a
    /// redirect would carry to the new host. With an `https` endpoint, it sends nothing over
    /// plain `http`. It ignores the proxy environment variables, like janky's own HTTP client.
    pub fn login(endpoint: &str, username: &str, password: &str) -> Result<Session> {
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().map_err(fail)?;
        let url = Url::parse(endpoint).map_err(fail)?;
        let client = Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .https_only(url.scheme() == "https")
            .no_proxy()
            .build()
            .map_err(fail)?;
        let mut auth = runtime
            .block_on(sync_login(username, password, Some(endpoint.to_string()), client.clone()))
            .map_err(sync_fail)?;
        auth.endpoint = Some(url);
        Ok(Session { auth, client, runtime })
    }

    /// Runs a normal sync of `col`. If the server asks for a full sync, the function replaces
    /// the copy with a full download instead. It returns the collection, open again after a
    /// full download.
    pub fn pull(&self, mut col: Collection) -> Result<(Collection, Pulled)> {
        let output =
            self.runtime.block_on(col.normal_sync(self.auth.clone(), self.client.clone())).map_err(sync_fail)?;
        if let SyncActionRequired::FullSyncRequired { .. } = output.required {
            let mut builder = col.as_builder();
            self.runtime.block_on(col.full_download(self.auth.clone(), self.client.clone())).map_err(sync_fail)?;
            return Ok((builder.build().map_err(fail)?, Pulled::FullDownload));
        }
        Ok((col, Pulled::Normal))
    }

    /// Runs a normal sync of `col` to send its changes. After a schema change, it sends nothing
    /// and returns [`Pushed::FullSyncRequired`].
    pub fn push(&self, col: &mut Collection) -> Result<Pushed> {
        let output =
            self.runtime.block_on(col.normal_sync(self.auth.clone(), self.client.clone())).map_err(sync_fail)?;
        Ok(match output.required {
            SyncActionRequired::FullSyncRequired { download_ok, .. } => {
                // rslib allows a download only when the server has cards or the copy has none.
                Pushed::FullSyncRequired { server_empty: !download_ok }
            },
            _ => Pushed::Done,
        })
    }

    /// Replaces the server's collection with `col`, and returns `col` open again.
    ///
    /// The other devices must then download the whole collection. Their changes that the server
    /// does not have yet are lost.
    pub fn full_upload(&self, col: Collection) -> Result<Collection> {
        let mut builder = col.as_builder();
        self.runtime.block_on(col.full_upload(self.auth.clone(), self.client.clone())).map_err(sync_fail)?;
        builder.build().map_err(fail)
    }

    /// Syncs the media folder of `col` in both directions.
    pub fn sync_media(&self, col: &Collection) -> Result<()> {
        let media = col.media().map_err(fail)?;
        self.runtime
            .block_on(media.sync_media(col.new_progress_handler(), self.auth.clone(), self.client.clone(), None))
            .map_err(sync_fail)
    }
}

/// The largest media file that the sync server accepts, in bytes.
pub const MEDIA_LIMIT: u64 = 100 * 1024 * 1024;

/// The number of files in one media download, as rslib's own client asks for.
const MEDIA_GROUP: usize = 25;

impl Session {
    /// The names of the media files that the server has now, sorted.
    pub fn media_names(&self) -> Result<Vec<String>> {
        let http = HttpSyncClient::new(self.auth.clone(), self.client.clone());
        self.runtime.block_on(async {
            let begin = SyncBeginRequest { client_version: CLIENT.into() }.try_into_sync_request().map_err(fail)?;
            http.begin(begin).await.map_err(|error| sync_fail(error.into()))?.json_result().map_err(sync_fail)?;
            let mut names = BTreeSet::new();
            let mut last_usn = Usn(0);
            loop {
                let request = MediaChangesRequest { last_usn }.try_into_sync_request().map_err(fail)?;
                let batch = http
                    .media_changes(request)
                    .await
                    .map_err(|error| sync_fail(error.into()))?
                    .json_result()
                    .map_err(sync_fail)?;
                let Some(last) = batch.last() else { break };
                last_usn = advance(last_usn, last.usn)?;
                // The changes come in usn order. An empty checksum means that the file was deleted.
                for change in batch {
                    if change.sha1.is_empty() {
                        names.remove(&change.fname);
                    } else {
                        names.insert(change.fname);
                    }
                }
            }
            Ok(names.into_iter().collect())
        })
    }

    /// Downloads the media files `names` from the server, in groups of 25, and gives each file
    /// to `each` before it downloads the next group.
    ///
    /// A file that janky did not ask for, a name that comes twice, or a file larger than
    /// [`MEDIA_LIMIT`] is an error.
    pub fn download_media(&self, names: &[String], mut each: impl FnMut(String, Vec<u8>)) -> Result<()> {
        let http = HttpSyncClient::new(self.auth.clone(), self.client.clone());
        for group in names.chunks(MEDIA_GROUP) {
            let request = DownloadFilesRequest { files: group.to_vec() }.try_into_sync_request().map_err(fail)?;
            let zip =
                self.runtime.block_on(http.download_files(request)).map_err(|error| sync_fail(error.into()))?.data;
            for (name, data) in unpack(&zip, group, MEDIA_LIMIT)? {
                each(name, data);
            }
        }
        Ok(())
    }
}

/// The next position in the paged media list. A page that does not move forward is an error, so
/// that a server cannot keep the loop going forever.
fn advance(last: Usn, next: Usn) -> Result<Usn> {
    if next.0 > last.0 {
        Ok(next)
    } else {
        Err(Error(format!("the server's media list did not move forward after {}", last.0)))
    }
}

/// The client name that the media sync sends. The server only needs it to be set.
const CLIENT: &str = concat!("janky,", env!("CARGO_PKG_VERSION"));

/// Reads a media zip from the server: entries named by number, and `_meta`, which maps each
/// number to the file name.
fn unpack(zip: &[u8], wanted: &[String], limit: u64) -> Result<Vec<(String, Vec<u8>)>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(fail)?;
    let names: HashMap<String, String> = {
        let meta = archive.by_name("_meta").map_err(fail)?;
        serde_json::from_reader(meta.take(1024 * 1024)).map_err(fail)?
    };
    if archive.len() > wanted.len() + 1 {
        return Err(Error(format!("the server sent {} files for {} names", archive.len() - 1, wanted.len())));
    }
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(fail)?;
        let entry = file.name().map_err(fail)?.to_string();
        if entry == "_meta" {
            continue;
        }
        let name = names.get(&entry).ok_or_else(|| fail("the server sent a file without a name"))?.clone();
        if !wanted.contains(&name) {
            return Err(Error(format!("the server sent `{name}`, which janky did not ask for")));
        }
        if out.iter().any(|(seen, _)| *seen == name) {
            return Err(Error(format!("the server sent `{name}` twice")));
        }
        let mut data = Vec::new();
        file.take(limit + 1).read_to_end(&mut data).map_err(fail)?;
        if data.len() as u64 > limit {
            return Err(Error(format!("the server sent `{name}` with more than {limit} bytes")));
        }
        out.push((name, data));
    }
    Ok(out)
}

fn sync_fail(error: AnkiError) -> Error {
    match &error {
        AnkiError::SyncError { source } if source.kind == SyncErrorKind::AuthFailed => {
            Error("the sync server refused the user name or the password".into())
        },
        AnkiError::SyncError { source } if source.kind == SyncErrorKind::ClockIncorrect => {
            Error("the clock of this computer differs from the sync server's clock by more than 5 minutes".into())
        },
        _ => Error(format!("sync failed: {}", error.message(&I18n::template_only()))),
    }
}

#[cfg(test)]
mod tests {
    use anki::error::{NetworkError, NetworkErrorKind, SyncError};

    use super::*;

    fn kind(kind: SyncErrorKind) -> String {
        sync_fail(AnkiError::SyncError {
            source: SyncError { info: "x".into(), kind },
        })
        .0
    }

    fn zip(entries: &[(&str, &[u8])], names: &str) -> Vec<u8> {
        use std::io::Write;
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.start_file("_meta", options).unwrap();
        writer.write_all(names.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn a_media_zip_gives_only_the_files_that_janky_asked_for() {
        let wanted = vec!["a.mp3".to_string()];
        let good = zip(&[("0", b"abc")], r#"{"0": "a.mp3"}"#);
        assert_eq!(unpack(&good, &wanted, 10).unwrap(), [("a.mp3".to_string(), b"abc".to_vec())]);
        let extra = zip(&[("0", b"abc"), ("1", b"x")], r#"{"0": "a.mp3", "1": "../b.mp3"}"#);
        let two = vec!["a.mp3".to_string(), "c.mp3".to_string()];
        assert!(unpack(&extra, &two, 10).unwrap_err().0.contains("did not ask for"));
        let large = zip(&[("0", b"abcd")], r#"{"0": "a.mp3"}"#);
        assert!(unpack(&large, &wanted, 3).unwrap_err().0.contains("more than 3 bytes"));
        let twice = zip(&[("0", b"a"), ("1", b"b")], r#"{"0": "a.mp3", "1": "a.mp3"}"#);
        assert!(unpack(&twice, &wanted, 10).unwrap_err().0.contains("files for 1 names"));
        let both = vec!["a.mp3".to_string(), "b.mp3".to_string()];
        let repeated = zip(&[("0", b"a"), ("1", b"b")], r#"{"0": "a.mp3", "1": "a.mp3"}"#);
        assert!(unpack(&repeated, &both, 10).unwrap_err().0.contains("`a.mp3` twice"));
    }

    #[test]
    fn the_media_list_must_move_forward() {
        assert_eq!(advance(Usn(0), Usn(5)).unwrap(), Usn(5));
        assert!(advance(Usn(5), Usn(5)).unwrap_err().0.contains("did not move forward after 5"));
        assert!(advance(Usn(5), Usn(3)).is_err());
    }

    #[test]
    fn sync_errors_get_plain_messages() {
        assert_eq!(kind(SyncErrorKind::AuthFailed), "the sync server refused the user name or the password");
        assert!(kind(SyncErrorKind::ClockIncorrect).starts_with("the clock of this computer"));
        assert!(kind(SyncErrorKind::ServerError).starts_with("sync failed: "));
        let timeout = NetworkError {
            info: String::new(),
            kind: NetworkErrorKind::Timeout,
        };
        let message = sync_fail(AnkiError::NetworkError { source: timeout }).0;
        assert!(message.starts_with("sync failed: ") && !message.contains("NetworkError"), "{message}");
    }
}
