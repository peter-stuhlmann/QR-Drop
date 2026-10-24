//! End-to-end tests: real server on localhost, real HTTP client.

use std::io::{Cursor, Read};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use qr_drop_lib::server::receive::Uploads;
use qr_drop_lib::server::{self, AppState, ServerHandle};
use qr_drop_lib::share::{
    new_id, EventSink, ShareConfig, ShareEvent, ShareManager, ShareSource, TextSink,
};

struct Harness {
    manager: Arc<ShareManager>,
    events: Arc<Mutex<Vec<ShareEvent>>>,
    /// Texts that would have gone into the clipboard.
    texts: Arc<Mutex<Vec<String>>>,
    uploads: Arc<Uploads>,
    server: ServerHandle,
    client: reqwest::Client,
}

impl Harness {
    async fn new(config: ShareConfig) -> Self {
        let manager = Arc::new(ShareManager::new(config));
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let sink: EventSink = Arc::new(move |e| ev.lock().unwrap().push(e));
        let texts = Arc::new(Mutex::new(Vec::new()));
        let tx = texts.clone();
        let text_sink: TextSink = Arc::new(move |t: &str| {
            tx.lock().unwrap().push(t.to_string());
            true
        });
        let uploads = Arc::new(Uploads::default());
        let server = server::start_on(
            AppState {
                manager: manager.clone(),
                sink,
                text_sink,
                uploads: uploads.clone(),
            },
            None,
            Ipv4Addr::LOCALHOST,
        )
        .await
        .unwrap();
        assert!(server::PORT_RANGE.contains(&server.port));
        Self {
            manager,
            events,
            texts,
            uploads,
            server,
            client: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.server.port)
    }

    /// Registers a share; returns (id, token).
    fn create(&self, source: ShareSource) -> (String, String) {
        let s = self
            .manager
            .create_with_id(new_id().unwrap(), source)
            .unwrap();
        (s.id, s.token)
    }

    fn share(&self, paths: &[PathBuf]) -> (String, String) {
        self.create(ShareSource::from_paths(paths, "2026-10-21").unwrap())
    }

    fn count(&self, pred: impl Fn(&ShareEvent) -> bool) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| pred(e))
            .count()
    }

    async fn status(&self, token: &str) -> (u16, String) {
        let res = self
            .client
            .get(self.url(&format!("/s/{token}/status")))
            .send()
            .await
            .unwrap();
        (res.status().as_u16(), res.text().await.unwrap())
    }

    async fn wait_for(&self, pred: impl Fn(&ShareEvent) -> bool) -> bool {
        for _ in 0..50 {
            if self.events.lock().unwrap().iter().any(&pred) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

#[tokio::test]
async fn full_download_with_headers_and_one_time_semantics() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Urlaubsfoto Ä 😀.jpg");
    let data = pattern(300_000);
    std::fs::write(&path, &data).unwrap();
    let (id, token) = h.share(&[path]);

    // Landing page.
    let page = h
        .client
        .get(h.url(&format!("/s/{token}")))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["cache-control"], "no-store");
    let html = page.text().await.unwrap();
    assert!(html.contains("\"valid\":true"));
    assert!(html.contains("\"hasPreview\":true"));
    assert!(!html.contains("__SHARE_DATA__"));
    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Connected { .. }))
            .await
    );

    // Download.
    let res = h
        .client
        .get(h.url(&format!("/s/{token}/file")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "image/jpeg");
    assert_eq!(res.headers()["content-length"], "300000");
    assert_eq!(res.headers()["accept-ranges"], "bytes");
    let cd = res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(cd.starts_with("attachment; filename=\"Urlaubsfoto _ _.jpg\""));
    assert!(cd.contains("filename*=UTF-8''Urlaubsfoto%20%C3%84%20%F0%9F%98%80.jpg"));
    let body = res.bytes().await.unwrap();
    assert_eq!(&body[..], &data[..]);

    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Done { id: d } if *d == id))
            .await
    );
    let events = h.events.lock().unwrap().clone();
    let last_progress = events
        .iter()
        .rev()
        .find_map(|e| match e {
            ShareEvent::Progress { sent, total, .. } => Some((*sent, *total)),
            _ => None,
        })
        .unwrap();
    assert_eq!(last_progress, (300_000, 300_000));

    // One-time download: the token is gone afterwards.
    let again = h
        .client
        .get(h.url(&format!("/s/{token}/file")))
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 404);
    let page = h
        .client
        .get(h.url(&format!("/s/{token}")))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 404);
    assert!(page.text().await.unwrap().contains("\"valid\":false"));
}

#[tokio::test]
async fn status_reports_a_finished_download_without_side_effects() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.pdf");
    std::fs::write(&path, pattern(10_000)).unwrap();
    let (_, token) = h.share(&[path]);
    let status = h.url(&format!("/s/{token}/status"));

    // Still valid: 204, and asking does not count as a visit.
    let res = h.client.get(&status).send().await.unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(res.headers()["cache-control"], "no-store");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(h.events.lock().unwrap().is_empty());

    // An interrupted (partial) download keeps the code alive ...
    let file = h.url(&format!("/s/{token}/file"));
    let res = h
        .client
        .get(&file)
        .header("Range", "bytes=0-99")
        .send()
        .await
        .unwrap();
    assert_eq!(res.bytes().await.unwrap().len(), 100);
    assert_eq!(h.client.get(&status).send().await.unwrap().status(), 204);

    // ... a complete one uses it up.
    let res = h.client.get(&file).send().await.unwrap();
    assert_eq!(res.bytes().await.unwrap().len(), 10_000);
    assert!(h.wait_for(|e| matches!(e, ShareEvent::Done { .. })).await);
    let res = h.client.get(&status).send().await.unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(res.text().await.unwrap(), r#"{"reason":"done"}"#);
}

#[tokio::test]
async fn range_requests_do_not_close_the_share() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let data = pattern(100_000);
    std::fs::write(&path, &data).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));

    let res = h
        .client
        .get(&url)
        .header("Range", "bytes=1000-1999")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 206);
    assert_eq!(res.headers()["content-range"], "bytes 1000-1999/100000");
    assert_eq!(res.headers()["content-length"], "1000");
    assert_eq!(&res.bytes().await.unwrap()[..], &data[1000..2000]);

    let res = h
        .client
        .get(&url)
        .header("Range", "bytes=-10")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 206);
    assert_eq!(&res.bytes().await.unwrap()[..], &data[99_990..]);

    let res = h
        .client
        .get(&url)
        .header("Range", "bytes=200000-")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 416);
    assert_eq!(res.headers()["content-range"], "bytes */100000");

    // Partial requests never count as the "one" download.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!h
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, ShareEvent::Done { .. })));
    let res = h.client.get(&url).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.bytes().await.unwrap().len(), 100_000);
}

#[tokio::test]
async fn head_request_sends_no_body_and_keeps_share() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    std::fs::write(&path, pattern(5000)).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));
    let res = h.client.head(&url).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-length"], "5000");
    assert_eq!(res.headers()["content-type"], "application/pdf");
    assert_eq!(h.client.get(&url).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn folder_is_streamed_as_zip() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Projekt");
    std::fs::create_dir_all(root.join("Unterordner")).unwrap();
    std::fs::write(root.join("liesmich.txt"), "Hallo ".repeat(10_000)).unwrap();
    std::fs::write(root.join("Unterordner/bild.png"), pattern(70_000)).unwrap();
    let (id, token) = h.share(&[root]);

    let res = h
        .client
        .get(h.url(&format!("/s/{token}/file")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "application/zip");
    assert!(res.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .contains("Projekt.zip"));
    let bytes = res.bytes().await.unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut text = String::new();
    zip.by_name("Projekt/liesmich.txt")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert_eq!(text, "Hallo ".repeat(10_000));
    let mut png = Vec::new();
    zip.by_name("Projekt/Unterordner/bild.png")
        .unwrap()
        .read_to_end(&mut png)
        .unwrap();
    assert_eq!(png, pattern(70_000));
    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Done { id: d } if *d == id))
            .await
    );
}

#[tokio::test]
async fn text_is_fetched_once_and_never_as_file() {
    let h = Harness::new(ShareConfig::default()).await;
    let (_, token) = h.create(ShareSource::Text {
        text: "Tr0ub4dor&3 </script><b>".into(),
        secret: true,
    });
    let url = h.url(&format!("/s/{token}"));

    assert_eq!(
        h.client
            .get(format!("{url}/file"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    // The page describes the text but does not contain it, and showing it
    // (link previews, reloads) does not use the share up.
    for _ in 0..2 {
        let page = h.client.get(&url).send().await.unwrap();
        assert_eq!(page.status(), 200);
        let html = page.text().await.unwrap();
        assert!(html.contains("\"isText\":true"));
        assert!(html.contains("\"secret\":true"));
        assert!(html.contains("\"chars\":24"));
        assert!(!html.contains("Tr0ub4dor"));
    }
    assert_eq!(h.status(&token).await.0, 204);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Done { .. })), 0);

    // Taking it hands it out exactly once.
    let take = h.client.post(format!("{url}/take")).send().await.unwrap();
    assert_eq!(take.status(), 200);
    assert_eq!(take.headers()["content-type"], "application/json");
    assert_eq!(take.headers()["cache-control"], "no-store");
    let body: serde_json::Value = serde_json::from_str(&take.text().await.unwrap()).unwrap();
    assert_eq!(body["text"], "Tr0ub4dor&3 </script><b>");
    assert!(h.wait_for(|e| matches!(e, ShareEvent::Done { .. })).await);

    let again = h.client.post(format!("{url}/take")).send().await.unwrap();
    assert_eq!(again.status(), 404);
    assert_eq!(h.status(&token).await, (404, r#"{"reason":"done"}"#.into()));
    let page = h.client.get(&url).send().await.unwrap();
    assert_eq!(page.status(), 404);
    assert!(page.text().await.unwrap().contains("\"valid\":false"));
}

#[tokio::test]
async fn links_are_not_served() {
    let h = Harness::new(ShareConfig::default()).await;
    let (_, token) = h.create(ShareSource::from_text("https://example.com").unwrap());
    let res = h
        .client
        .get(h.url(&format!("/s/{token}")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn unknown_paths_and_tokens_are_404_without_details() {
    let h = Harness::new(ShareConfig::default()).await;
    for path in [
        "/",
        "/s",
        "/s/abc/file",
        "/s/abc/preview",
        "/etc/passwd",
        "/s/../x",
    ] {
        let res = h.client.get(h.url(path)).send().await.unwrap();
        assert_eq!(res.status(), 404, "{path}");
    }
}

#[tokio::test]
async fn expired_token_is_rejected() {
    let h = Harness::new(ShareConfig {
        ttl: Duration::from_millis(50),
        ..ShareConfig::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"abc").unwrap();
    let (_, token) = h.share(&[path]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let res = h
        .client
        .get(h.url(&format!("/s/{token}/file")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn preview_is_a_small_jpeg() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gross.png");
    let img = image::RgbImage::from_fn(2400, 1600, |x, y| image::Rgb([x as u8, y as u8, 128]));
    img.save(&path).unwrap();
    let (_, token) = h.share(&[path]);
    let res = h
        .client
        .get(h.url(&format!("/s/{token}/preview")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "image/jpeg");
    let jpeg = res.bytes().await.unwrap();
    let decoded = image::load_from_memory(&jpeg).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (1200, 800));
}

/// Streams a > 4 GB sparse file without loading it into memory.
/// Slow (reads 4.3 GB over loopback), so run explicitly:
/// `cargo test --release -- --ignored huge_file`
#[tokio::test]
#[ignore]
async fn huge_file_over_4gb() {
    use futures_util::StreamExt;
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("riesig.bin");
    let size: u64 = 4 * 1024 * 1024 * 1024 + 123;
    std::fs::File::create(&path).unwrap().set_len(size).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));

    let res = h
        .client
        .get(&url)
        .header("Range", "bytes=4294967296-")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 206);
    assert_eq!(res.bytes().await.unwrap().len(), 123);

    let res = h.client.get(&url).send().await.unwrap();
    assert_eq!(res.headers()["content-length"], size.to_string());
    let mut stream = res.bytes_stream();
    let mut total = 0u64;
    while let Some(chunk) = stream.next().await {
        total += chunk.unwrap().len() as u64;
    }
    assert_eq!(total, size);
}

// ---------------------------------------------------------------------------
// Phone → computer
// ---------------------------------------------------------------------------

impl Harness {
    fn receive_into(&self, dir: &std::path::Path) -> (String, String) {
        self.create(ShareSource::Receive {
            dir: dir.to_path_buf(),
        })
    }

    async fn upload(&self, token: &str, name: &str, body: Vec<u8>) -> reqwest::Response {
        self.client
            .post(self.url(&format!("/s/{token}/upload")))
            .query(&[("name", name), ("i", "1"), ("n", "1")])
            .body(body)
            .send()
            .await
            .unwrap()
    }
}

/// Every file name in a directory (including hidden temp files).
fn dir_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn receive_page_and_uploads_land_safely_in_the_folder() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("Downloads");
    std::fs::create_dir(&target).unwrap();
    let (id, token) = h.receive_into(&target);

    // The phone opens the upload page.
    let page = h
        .client
        .get(h.url(&format!("/s/{token}")))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    let csp = page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(csp.contains("'sha256-") && !csp.contains("unsafe-inline"));
    let html = page.text().await.unwrap();
    assert!(html.contains("\"folder\":\"Downloads\""));
    assert!(!html.contains("__SHARE_DATA__") && !html.contains("__COMMON_JS__"));
    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Connected { .. }))
            .await
    );

    // A normal photo.
    let photo = pattern(250_000);
    let res = h.upload(&token, "IMG_2041.HEIC", photo.clone()).await;
    assert_eq!(res.status(), 201);
    assert_eq!(res.text().await.unwrap(), r#"{"name":"IMG_2041.HEIC"}"#);
    assert_eq!(std::fs::read(target.join("IMG_2041.HEIC")).unwrap(), photo);

    // Same name again: never overwritten.
    let res = h.upload(&token, "IMG_2041.HEIC", b"second".to_vec()).await;
    assert_eq!(res.text().await.unwrap(), r#"{"name":"IMG_2041 (2).HEIC"}"#);
    assert_eq!(std::fs::read(target.join("IMG_2041.HEIC")).unwrap(), photo);

    // Path tricks end up as a plain name inside the folder.
    let res = h.upload(&token, "../../evil.txt", b"x".to_vec()).await;
    assert_eq!(res.text().await.unwrap(), r#"{"name":"evil.txt"}"#);
    let res = h.upload(&token, "..\\..\\CON.txt", b"y".to_vec()).await;
    assert_eq!(res.text().await.unwrap(), r#"{"name":"_CON.txt"}"#);
    assert!(!dir.path().join("evil.txt").exists());
    assert_eq!(
        dir_names(&target),
        ["IMG_2041 (2).HEIC", "IMG_2041.HEIC", "_CON.txt", "evil.txt"]
    );

    // Events: progress and one "received" per file, with the saved name.
    let events = h.events.lock().unwrap().clone();
    let received: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ShareEvent::Received {
                id: i, name, size, ..
            } if *i == id => Some((name.clone(), *size)),
            _ => None,
        })
        .collect();
    assert_eq!(received[0], ("IMG_2041.HEIC".to_string(), 250_000));
    assert_eq!(received.len(), 4);
    assert!(events.iter().any(|e| matches!(
        e,
        ShareEvent::Upload {
            sent: 250_000,
            total: 250_000,
            ..
        }
    )));

    // A receive session never serves files and stays open for more.
    let file = h.client.get(h.url(&format!("/s/{token}/file"))).send();
    assert_eq!(file.await.unwrap().status(), 404);
    let status = h.client.get(h.url(&format!("/s/{token}/status"))).send();
    assert_eq!(status.await.unwrap().status(), 204);
}

#[tokio::test]
async fn interrupted_upload_leaves_nothing_behind() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());

    // Announce 1 MB, send 1 KB, hang up.
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", h.server.port))
        .await
        .unwrap();
    let head = format!(
        "POST /s/{token}/upload?name=video.mp4 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 1000000\r\n\r\n"
    );
    tcp.write_all(head.as_bytes()).await.unwrap();
    tcp.write_all(&[7u8; 1024]).await.unwrap();
    tcp.shutdown().await.unwrap();
    let mut rest = Vec::new();
    let _ = tcp.read_to_end(&mut rest).await;
    drop(tcp);

    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Interrupted { .. }))
            .await
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        dir_names(dir.path()).is_empty(),
        "{:?}",
        dir_names(dir.path())
    );
    assert!(!h
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, ShareEvent::Received { .. })));

    // The session is still usable afterwards.
    let res = h.upload(&token, "video.mp4", pattern(10)).await;
    assert_eq!(res.status(), 201);
}

#[tokio::test]
async fn texts_from_the_phone_go_to_the_clipboard() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let url = h.url(&format!("/s/{token}/text"));

    let res = h
        .client
        .post(&url)
        .body("Hallo vom Handy 👋\nzweite Zeile")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    let res = h
        .client
        .post(&url)
        .body("Tr0ub4dor&3")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(
        *h.texts.lock().unwrap(),
        ["Hallo vom Handy 👋\nzweite Zeile", "Tr0ub4dor&3"]
    );

    let previews: Vec<_> = h
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            ShareEvent::TextReceived { chars, preview, .. } => Some((*chars, preview.clone())),
            _ => None,
        })
        .collect();
    // Passwords never show up in the window.
    assert_eq!(
        previews,
        [(30, Some("Hallo vom Handy 👋".to_string())), (11, None)]
    );

    let empty = h.client.post(&url).body("  \n ").send().await.unwrap();
    assert_eq!(empty.status(), 400);
    let huge = h
        .client
        .post(&url)
        .body("x".repeat(3 * 1024 * 1024))
        .send()
        .await
        .unwrap();
    assert_eq!(huge.status(), 413);
    assert_eq!(huge.text().await.unwrap(), r#"{"error":"too_large"}"#);
    assert_eq!(h.texts.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn uploads_need_a_receive_session() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    std::fs::write(&path, pattern(100)).unwrap();
    let (_, share_token) = h.share(&[path]);

    // A download token cannot be used to write files or texts.
    let res = h.upload(&share_token, "x.txt", b"x".to_vec()).await;
    assert_eq!(res.status(), 404);
    let res = h
        .client
        .post(h.url(&format!("/s/{share_token}/text")))
        .body("x")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    let res = h
        .upload("AAAAAAAAAAAAAAAAAAAAAA", "x.txt", b"x".to_vec())
        .await;
    assert_eq!(res.status(), 404);
    assert_eq!(dir_names(dir.path()), ["a.pdf"]);
    assert!(h.texts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn closing_the_window_stops_a_running_upload() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (id, token) = h.receive_into(dir.path());

    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", h.server.port))
        .await
        .unwrap();
    let head = format!(
        "POST /s/{token}/upload?name=film.mp4 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 4000000\r\n\r\n"
    );
    tcp.write_all(head.as_bytes()).await.unwrap();
    tcp.write_all(&[1u8; 64 * 1024]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

    // "Abbrechen" on the computer.
    h.manager.remove(&id);
    let _ = tcp.write_all(&[2u8; 256 * 1024]).await;
    let mut answer = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), tcp.read_to_end(&mut answer)).await;
    let answer = String::from_utf8_lossy(&answer);
    assert!(answer.starts_with("HTTP/1.1 410"), "{answer}");
    assert!(answer.contains("cancelled"));

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        dir_names(dir.path()).is_empty(),
        "{:?}",
        dir_names(dir.path())
    );
    assert!(!h
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, ShareEvent::Received { .. })));
}

#[tokio::test]
async fn check_refuses_files_that_do_not_fit() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let check = |size: u64| {
        h.client
            .get(h.url(&format!("/s/{token}/check")))
            .query(&[("size", size)])
            .send()
    };
    assert_eq!(check(1000).await.unwrap().status(), 204);
    let res = check(u64::MAX / 2).await.unwrap();
    assert_eq!(res.status(), 507);
    assert_eq!(res.text().await.unwrap(), r#"{"error":"disk_full"}"#);

    // Only for receive sessions.
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"a").unwrap();
    let (_, share_token) = h.share(&[path]);
    let res = h
        .client
        .get(h.url(&format!("/s/{share_token}/check?size=1")))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
}

/// Sends the first `cut` bytes of an upload of `data`, then hangs up (like a
/// phone that got locked).
async fn interrupted_upload(h: &Harness, token: &str, key: &str, data: &[u8], cut: usize) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", h.server.port))
        .await
        .unwrap();
    let head = format!(
        "POST /s/{token}/upload?name=film.mp4&key={key}&size={} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n",
        data.len(),
        data.len()
    );
    tcp.write_all(head.as_bytes()).await.unwrap();
    tcp.write_all(&data[..cut]).await.unwrap();
    tcp.shutdown().await.unwrap();
    let mut rest = Vec::new();
    let _ = tcp.read_to_end(&mut rest).await;
    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Interrupted { .. }))
            .await
    );
}

async fn offset_of(h: &Harness, token: &str, key: &str) -> serde_json::Value {
    let text = h
        .client
        .get(h.url(&format!("/s/{token}/upload")))
        .query(&[("key", key)])
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn interrupted_uploads_resume_where_they_stopped() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let data = pattern(600_000);
    let key = "k3yForFilm01";

    // What was written to disk before the connection broke is kept; the
    // unwritten rest in memory is dropped and simply sent again.
    interrupted_upload(&h, &token, key, &data, 400_000).await;
    let offset = offset_of(&h, &token, key).await["offset"].as_u64().unwrap();
    assert!((256 * 1024..=400_000).contains(&offset), "{offset}");
    // Only the hidden part file, no finished file yet.
    let names = dir_names(dir.path());
    assert_eq!(names.len(), 1);
    assert!(names[0].starts_with(".qrdrop-") && names[0].ends_with(".part"));

    // Resuming at the wrong position is refused with the right one.
    let wrong = h
        .client
        .post(h.url(&format!("/s/{token}/upload")))
        .query(&[
            ("name", "film.mp4"),
            ("key", key),
            ("size", "600000"),
            ("from", "50"),
        ])
        // Small body: the answer comes before the server reads it.
        .body(data[50..60].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 409);
    assert_eq!(
        wrong.text().await.unwrap(),
        format!(r#"{{"error":"offset","offset":{offset}}}"#)
    );

    // The rest completes the file.
    let res = h
        .client
        .post(h.url(&format!("/s/{token}/upload")))
        .query(&[
            ("name", "film.mp4"),
            ("key", key),
            ("size", "600000"),
            ("from", &offset.to_string()),
        ])
        .body(data[offset as usize..].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    assert_eq!(res.text().await.unwrap(), r#"{"name":"film.mp4"}"#);
    assert_eq!(dir_names(dir.path()), ["film.mp4"]);
    assert_eq!(std::fs::read(dir.path().join("film.mp4")).unwrap(), data);

    let received: Vec<u64> = h
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            ShareEvent::Received { size, .. } => Some(*size),
            _ => None,
        })
        .collect();
    assert_eq!(received, [600_000]);

    // Afterwards the key is known as done: asking or sending again (a retry
    // whose answer got lost) never saves a second copy.
    let done = offset_of(&h, &token, key).await;
    assert_eq!(
        done,
        serde_json::json!({ "offset": 600_000, "done": true, "name": "film.mp4" })
    );
    let retry = h
        .client
        .post(h.url(&format!("/s/{token}/upload")))
        .query(&[("name", "film.mp4"), ("key", key), ("size", "600000")])
        // Small body: the answer comes before the server reads it.
        .body(data[..10].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(retry.status(), 201);
    assert_eq!(retry.text().await.unwrap(), r#"{"name":"film.mp4"}"#);
    assert_eq!(dir_names(dir.path()), ["film.mp4"]);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Received { .. })), 1);
}

#[tokio::test]
async fn unknown_keys_start_at_zero_and_bad_keys_are_not_kept() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());

    let res = h
        .client
        .post(h.url(&format!("/s/{token}/upload")))
        .query(&[("name", "a.txt"), ("key", "neverSeen99"), ("from", "10")])
        .body(b"x".to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 409);
    assert_eq!(
        res.text().await.unwrap(),
        r#"{"error":"offset","offset":0}"#
    );

    // A key with path characters is ignored: the upload is not resumable,
    // so nothing stays behind after an interruption.
    interrupted_upload(&h, &token, "..%2F..%2Fx", &pattern(50_000), 1_000).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        dir_names(dir.path()).is_empty(),
        "{:?}",
        dir_names(dir.path())
    );
}

#[tokio::test]
async fn closing_the_window_discards_unfinished_uploads() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (id, token) = h.receive_into(dir.path());
    interrupted_upload(&h, &token, "k3yForFilm02", &pattern(80_000), 20_000).await;
    assert_eq!(dir_names(dir.path()).len(), 1);

    h.manager.remove(&id);
    h.uploads.sweep(&h.manager);
    assert!(dir_names(dir.path()).is_empty());
}

// ---------------------------------------------------------------------------
// Lifetime, cancellation and concurrency
// ---------------------------------------------------------------------------

/// Large enough that the server is still streaming while the test reacts
/// (more than the loopback socket buffers hold).
const BIG: usize = 32 * 1024 * 1024;

/// Reads the rest of a streamed body; returns the byte count and whether the
/// body ended with an error (the server aborted it).
async fn drain(
    stream: &mut (impl futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Unpin),
) -> (usize, bool) {
    use futures_util::StreamExt;
    let mut total = 0;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(c) => total += c.len(),
            Err(_) => return (total, true),
        }
    }
    (total, false)
}

#[tokio::test]
async fn closing_the_window_stops_a_running_download() {
    use futures_util::StreamExt;
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("film.mp4");
    std::fs::write(&path, pattern(BIG)).unwrap();
    let (id, token) = h.share(&[path]);

    let res = h
        .client
        .get(h.url(&format!("/s/{token}/file")))
        .send()
        .await
        .unwrap();
    let mut stream = res.bytes_stream();
    let first = stream.next().await.unwrap().unwrap().len();
    h.manager.remove(&id);
    let (rest, _) = drain(&mut stream).await;
    assert!(first + rest < BIG, "the download must stop");
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Done { .. })), 0);
}

#[tokio::test]
async fn a_running_download_outlives_the_expiry_and_completes() {
    use futures_util::StreamExt;
    let h = Harness::new(ShareConfig {
        ttl: Duration::from_millis(300),
        ..ShareConfig::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("film.mp4");
    std::fs::write(&path, pattern(BIG)).unwrap();
    let (id, token) = h.share(&[path]);

    let res = h
        .client
        .get(h.url(&format!("/s/{token}/file")))
        .send()
        .await
        .unwrap();
    let mut stream = res.bytes_stream();
    let first = stream.next().await.unwrap().unwrap().len();
    tokio::time::sleep(Duration::from_millis(400)).await;
    // Past its lifetime, but busy: not expired.
    assert!(h.manager.take_expired(Instant::now()).is_empty());
    // The share stays reachable while the download runs.
    let late = h.client.get(h.url(&format!("/s/{token}/file")));
    assert_eq!(
        late.header("Range", "bytes=0-0")
            .send()
            .await
            .unwrap()
            .status(),
        206
    );

    let (rest, failed) = drain(&mut stream).await;
    assert!(!failed);
    assert_eq!(first + rest, BIG);
    assert!(
        h.wait_for(|e| matches!(e, ShareEvent::Done { id: d } if *d == id))
            .await
    );
    assert!(h.manager.take_expired(Instant::now()).is_empty());
    assert_eq!(h.status(&token).await, (404, r#"{"reason":"done"}"#.into()));
}

#[tokio::test]
async fn of_parallel_downloads_only_one_gets_the_single_copy() {
    use futures_util::StreamExt;
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("film.mp4");
    std::fs::write(&path, pattern(BIG)).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));

    let a = h.client.get(&url).send().await.unwrap();
    let b = h.client.get(&url).send().await.unwrap();
    let (mut a, mut b) = (a.bytes_stream(), b.bytes_stream());
    let b_first = b.next().await.unwrap().unwrap().len();

    let (a_total, a_failed) = drain(&mut a).await;
    assert!(!a_failed);
    assert_eq!(a_total, BIG);
    assert!(h.wait_for(|e| matches!(e, ShareEvent::Done { .. })).await);

    let (b_rest, _) = drain(&mut b).await;
    assert!(b_first + b_rest < BIG, "the second download is stopped");
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Done { .. })), 1);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Interrupted { .. })), 0);
}

#[tokio::test]
async fn ranges_alone_complete_a_share_and_stay_usable_briefly() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let data = pattern(100_000);
    std::fs::write(&path, &data).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));
    let range = |r: &'static str| h.client.get(&url).header("Range", r).send();

    assert_eq!(range("bytes=0-49999").await.unwrap().status(), 206);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Done { .. })), 0);
    let res = range("bytes=50000-").await.unwrap();
    assert_eq!(&res.bytes().await.unwrap()[..], &data[50_000..]);
    assert!(h.wait_for(|e| matches!(e, ShareEvent::Done { .. })).await);

    // The player may re-request the end right after; a full copy is gone.
    let again = range("bytes=99990-").await.unwrap();
    assert_eq!(again.status(), 206);
    assert_eq!(&again.bytes().await.unwrap()[..], &data[99_990..]);
    assert_eq!(h.client.get(&url).send().await.unwrap().status(), 404);
    assert_eq!(range("bytes=0-").await.unwrap().status(), 404);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Done { .. })), 1);
}

#[tokio::test]
async fn a_download_that_completes_earlier_ranges_is_sent_to_the_end() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mp4");
    let data = pattern(300_000);
    std::fs::write(&path, &data).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));

    // The tail first (players probe the end), then everything: the share is
    // complete halfway through the full download, which still finishes.
    let tail = h.client.get(&url).header("Range", "bytes=100000-").send();
    assert_eq!(tail.await.unwrap().bytes().await.unwrap().len(), 200_000);
    let full = h.client.get(&url).send().await.unwrap();
    assert_eq!(full.status(), 200);
    assert_eq!(&full.bytes().await.unwrap()[..], &data[..]);
    assert!(h.wait_for(|e| matches!(e, ShareEvent::Done { .. })).await);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Done { .. })), 1);
}

#[tokio::test]
async fn if_range_only_resumes_the_same_version() {
    // Several downloads allowed, so every case gets a living share.
    let h = Harness::new(ShareConfig {
        single_download: false,
        ..ShareConfig::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.pdf");
    std::fs::write(&path, pattern(10_000)).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}/file"));

    let head = h.client.head(&url).send().await.unwrap();
    let etag = head.headers()["etag"].to_str().unwrap().to_string();
    let date = head.headers()["last-modified"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(etag.starts_with("W/\""), "{etag}");
    assert_eq!(
        head.headers()["content-security-policy"],
        "sandbox; default-src 'none'"
    );

    let same = h
        .client
        .get(&url)
        .header("Range", "bytes=0-9")
        .header("If-Range", &date)
        .send()
        .await
        .unwrap();
    assert_eq!(same.status(), 206);

    // A changed (or only weakly identified) version: everything again.
    for validator in [etag.as_str(), "Sun, 06 Nov 1994 08:49:37 GMT"] {
        let other = h
            .client
            .get(&url)
            .header("Range", "bytes=0-9")
            .header("If-Range", validator)
            .send()
            .await
            .unwrap();
        assert_eq!(other.status(), 200, "{validator}");
        assert_eq!(other.headers()["content-length"], "10000");
    }
}

#[tokio::test]
async fn status_explains_why_a_code_is_gone() {
    let h = Harness::new(ShareConfig {
        ttl: Duration::from_millis(50),
        ..ShareConfig::default()
    })
    .await;
    assert_eq!(
        h.status("AAAAAAAAAAAAAAAAAAAAAA").await,
        (404, r#"{"reason":"unknown"}"#.into())
    );
    let (id, token) = h.create(ShareSource::from_text("Hallo Welt").unwrap());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        h.status(&token).await,
        (404, r#"{"reason":"expired"}"#.into())
    );
    assert_eq!(h.manager.take_expired(Instant::now()), vec![id.clone()]);
    assert_eq!(
        h.status(&token).await,
        (404, r#"{"reason":"expired"}"#.into())
    );
    h.manager.remove(&id);
    assert_eq!(
        h.status(&token).await,
        (404, r#"{"reason":"unknown"}"#.into())
    );
    // Asking never counts as a visit.
    assert!(h.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn requests_under_a_foreign_host_name_are_refused() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    std::fs::write(&path, pattern(100)).unwrap();
    let (_, token) = h.share(&[path]);
    let url = h.url(&format!("/s/{token}"));

    // DNS rebinding: a page on some domain pointed at this computer.
    let res = h
        .client
        .get(&url)
        .header("Host", "evil.example:8080")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 403);
    assert_eq!(res.headers()["cache-control"], "no-store");
    let res = h
        .client
        .get(&url)
        .header("Host", format!("localhost:{}", h.server.port))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn landing_page_csp_allows_only_its_own_code() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.pdf");
    std::fs::write(&path, pattern(100)).unwrap();
    let (_, token) = h.share(&[path]);
    let page = h
        .client
        .get(h.url(&format!("/s/{token}")))
        .send()
        .await
        .unwrap();
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(csp.contains("script-src 'sha256-"), "{csp}");
    assert!(
        !csp.contains("unsafe-inline") && !csp.contains("data:"),
        "{csp}"
    );
    assert_eq!(page.headers()["referrer-policy"], "no-referrer");
    assert_eq!(page.headers()["x-content-type-options"], "nosniff");
}

#[tokio::test]
async fn receive_sessions_idle_out_and_end_after_the_limit() {
    let h = Harness::new(ShareConfig {
        ttl: Duration::from_millis(400),
        max_lifetime: Duration::from_millis(1200),
        ..ShareConfig::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();

    // Opening the page or asking for space is no activity.
    let (_, idle) = h.receive_into(dir.path());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let page = h.client.get(h.url(&format!("/s/{idle}"))).send();
    assert_eq!(page.await.unwrap().status(), 200);
    let check = h
        .client
        .get(h.url(&format!("/s/{idle}/check?size=1")))
        .send();
    assert_eq!(check.await.unwrap().status(), 204);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.status(&idle).await,
        (404, r#"{"reason":"expired"}"#.into())
    );

    // Received data is, but only up to the absolute limit.
    let (_, busy) = h.receive_into(dir.path());
    let started = Instant::now();
    let text = h.url(&format!("/s/{busy}/text"));
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let res = h.client.post(&text).body("Hallo").send().await.unwrap();
        assert_eq!(res.status(), 204, "after {:?}", started.elapsed());
    }
    tokio::time::sleep(Duration::from_millis(1300).saturating_sub(started.elapsed())).await;
    let res = h.client.post(&text).body("Hallo").send().await.unwrap();
    assert_eq!(res.status(), 404);
}

// ---------------------------------------------------------------------------
// Uploads: takeover, limits, names
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_new_request_takes_over_a_stalled_upload() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let data = pattern(600_000);
    let key = "takeOver01";

    // A: the phone lost the Wi-Fi mid-upload, but the connection lingers.
    let mut a = tokio::net::TcpStream::connect(("127.0.0.1", h.server.port))
        .await
        .unwrap();
    let head = format!(
        "POST /s/{token}/upload?name=film.mp4&key={key}&size=600000 HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 600000\r\n\r\n"
    );
    a.write_all(head.as_bytes()).await.unwrap();
    a.write_all(&data[..300_000]).await.unwrap();
    let mut offset = 0;
    for _ in 0..100 {
        offset = offset_of(&h, &token, key).await["offset"].as_u64().unwrap();
        if offset > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!((256 * 1024..=300_000).contains(&offset), "{offset}");

    // B: the phone retries from what the computer has.
    let b = h
        .client
        .post(h.url(&format!("/s/{token}/upload")))
        .query(&[
            ("name", "film.mp4"),
            ("key", key),
            ("size", "600000"),
            ("from", &offset.to_string()),
        ])
        .body(data[offset as usize..].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(b.status(), 201);
    assert_eq!(b.text().await.unwrap(), r#"{"name":"film.mp4"}"#);

    // A wakes up and sends the rest: it no longer owns the file.
    let _ = a.write_all(&data[300_000..]).await;
    let mut answer = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), a.read_to_end(&mut answer)).await;
    let answer = String::from_utf8_lossy(&answer);
    assert!(answer.starts_with("HTTP/1.1 409"), "{answer}");
    assert!(answer.contains("superseded"), "{answer}");

    assert_eq!(dir_names(dir.path()), ["film.mp4"]);
    assert_eq!(std::fs::read(dir.path().join("film.mp4")).unwrap(), data);
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Received { .. })), 1);
}

#[tokio::test]
async fn a_short_upload_without_key_leaves_nothing() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let res = h
        .client
        .post(h.url(&format!("/s/{token}/upload")))
        .query(&[("name", "a.txt"), ("size", "1000")])
        .body(pattern(500))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    assert_eq!(res.text().await.unwrap(), r#"{"error":"incomplete"}"#);
    assert!(
        dir_names(dir.path()).is_empty(),
        "{:?}",
        dir_names(dir.path())
    );
    assert_eq!(h.count(|e| matches!(e, ShareEvent::Received { .. })), 0);
}

#[tokio::test]
async fn too_many_unfinished_uploads_are_refused() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let start = |key: String| {
        h.client
            .post(h.url(&format!("/s/{token}/upload")))
            .query(&[("name", "a.txt"), ("key", key.as_str()), ("from", "5")])
            .body("x")
            .send()
    };
    for i in 0..64 {
        assert_eq!(start(format!("tooMany{i:03}")).await.unwrap().status(), 409);
    }
    let res = start("tooMany064".into()).await.unwrap();
    assert_eq!(res.status(), 429);
    assert_eq!(res.text().await.unwrap(), r#"{"error":"too_many"}"#);
}

#[tokio::test]
async fn invisible_characters_are_stripped_from_received_names() {
    let h = Harness::new(ShareConfig::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let (_, token) = h.receive_into(dir.path());
    let rlo = char::from_u32(0x202E).unwrap();
    let res = h
        .upload(&token, &format!("photo{rlo}gpj.exe"), b"MZ".to_vec())
        .await;
    assert_eq!(res.status(), 201);
    assert_eq!(res.text().await.unwrap(), r#"{"name":"photogpj.exe"}"#);
    assert_eq!(dir_names(dir.path()), ["photogpj.exe"]);
}
