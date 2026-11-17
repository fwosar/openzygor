use axum::{
    Router,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use openzygor::{
    api::{Api, DownloadRequest},
    config::{self, Config},
    discovery,
    installer::{self, PreparedInstall},
    model::{Game, Region},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;
use zip::{ZipWriter, write::SimpleFileOptions};

const PRODUCT: &str = "ZygorGuidesViewer";
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn full_archive(revision: &str) -> Vec<u8> {
    archive(&[
        (
            "ZygorGuidesViewer/ZygorGuidesViewer.toc",
            b"## Version: 9.6\n",
        ),
        (
            "ZygorGuidesViewer/Ver.lua",
            format!("$Revision: {revision} $").as_bytes(),
        ),
        ("ZygorGuidesViewer/Guide.lua", b"new guide"),
    ])
}
fn game_folder(root: &std::path::Path) -> openzygor::config::Installation {
    let path = root.join("World of Warcraft/_retail_");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("Wow.exe"), b"test executable placeholder").unwrap();
    discovery::validate(Game::Retail, &path).unwrap()
}
fn installed(root: &std::path::Path) -> openzygor::config::Installation {
    let i = game_folder(root);
    let addon = i.addons().join(PRODUCT);
    fs::create_dir_all(addon.join("Custom")).unwrap();
    fs::write(addon.join("ZygorGuidesViewer.toc"), b"## Version: 9.6\n").unwrap();
    fs::write(addon.join("Ver.lua"), b"$Revision: 100 $").unwrap();
    fs::write(addon.join("Guide.lua"), b"old guide").unwrap();
    fs::write(addon.join("obsolete.lua"), b"old").unwrap();
    fs::write(addon.join("Custom/MyGuide.lua"), b"my custom guide").unwrap();
    i
}

#[derive(Clone)]
struct Service {
    calls: Arc<Mutex<Vec<String>>>,
    archive: Vec<u8>,
}
async fn service(
    State(state): State<Service>,
    method: Method,
    headers: HeaderMap,
    Query(q): Query<BTreeMap<String, String>>,
    body: Bytes,
) -> Response {
    assert_eq!(headers["x-zygor-api"], "4");
    assert_eq!(headers["x-zygor-uv"], "4.13.2");
    assert_eq!(headers["x-zygor-os"], "linux");
    assert!(
        headers["user-agent"]
            .to_str()
            .unwrap()
            .starts_with("OpenZygor/")
    );
    assert_eq!(q["full"], "1");
    assert_eq!(q["wwwdev"], "0");
    let action = q.get("do").unwrap().as_str();
    state.calls.lock().unwrap().push(action.into());
    match action {
        "validate" => {
            assert_eq!(method, Method::GET);
            if let Some(user) = q.get("user") {
                assert_eq!(user, "fixture-user");
                // UTF-8 + base64 with the official client's Qö prefix, not a password hash.
                assert_eq!(q["pass"], "QöcMOkc3Mrd29yZCAmPw==");
                assert_eq!(q["auto"], "false");
            } else {
                assert_eq!(q["token"], "test-token+&?");
                assert_eq!(q["auto"], "true");
            }
            axum::Json(json!({"success":true,"user":{"login":"fixture-user","displayname":"Fixture User","token":"test-token+&?"},"access":{"level":"elite","status":"active"}})).into_response()
        }
        "list_packages" => {
            assert_eq!(method, Method::GET);
            assert_eq!(q["token"], "test-token+&?");
            assert_eq!(q["region"], "eu");
            let versions: serde_json::Value = serde_json::from_str(&q["installed"]).unwrap();
            assert_eq!(versions["wow"], "9.6.100");
            axum::Json(json!({"success":true,"packages":[{"game":"wow","name":PRODUCT,"ver":"9.6.101","status":{"available":true,"hidden":false}}]})).into_response()
        }
        "comp" => {
            assert_eq!(method, Method::POST);
            assert_eq!(q["package"], PRODUCT);
            assert_eq!(q["token"], "test-token+&?");
            assert_eq!(q["region"], "eu");
            assert_eq!(q["from"], "9.6.100");
            assert!(
                headers["content-type"]
                    .to_str()
                    .unwrap()
                    .starts_with("application/x-www-form-urlencoded")
            );
            let form: Vec<_> = reqwest::Url::parse(&format!(
                "http://localhost/?{}",
                String::from_utf8(body.to_vec()).unwrap()
            ))
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
            assert_eq!(form[0].0, "list");
            use md5::{Digest, Md5};
            let expected = format!(
                "F ZygorGuidesViewer/Guide.lua##{:x}\r\n",
                Md5::digest(b"old guide")
            );
            assert!(form[0].1.contains(&expected));
            assert!(form[0].1.contains("ZygorGuidesViewer/Custom/MyGuide.lua##"));
            (
                [
                    ("content-type", "application/zip"),
                    ("x-zygor-guides-version", "9.6.101"),
                ],
                state.archive,
            )
                .into_response()
        }
        "license" => {
            assert_eq!(q["package"], PRODUCT);
            assert_eq!(q["token"], "test-token+&?");
            "-- synthetic fixture licence\nZGV.TestLicense=true".into_response()
        }
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

#[tokio::test]
async fn login_refresh_catalogue_download_and_transactional_install() {
    let tmp = tempfile::tempdir().unwrap();
    let i = installed(tmp.path());
    let calls = Arc::new(Mutex::new(Vec::new()));
    let zip = archive(&[
        ("ZygorGuidesViewer/Ver.lua", b"$Revision: 101 $"),
        ("ZygorGuidesViewer/Guide.lua", b"new guide"),
        (
            ".zygor-meta",
            br#"{"DELETE":["ZygorGuidesViewer/obsolete.lua"]}"#,
        ),
    ]);
    let router = Router::new()
        .route("/packager.php", any(service))
        .with_state(Service {
            calls: calls.clone(),
            archive: zip,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api::new(&format!("http://{address}/packager.php")).unwrap();
    let login = api.login("fixture-user", "päss+word &?").await.unwrap();
    assert_eq!(login.account.display_name, "Fixture User");
    assert_eq!(login.account.access.level, "elite");
    let refreshed = api.validate(&login.token).await.unwrap();
    let catalogue = api
        .catalogue(
            &refreshed.token,
            Region::Eu,
            &BTreeMap::from([("wow".into(), Some("9.6.100".into()))]),
        )
        .await
        .unwrap();
    assert!(catalogue.packages[0].status.available);
    let cancel = CancellationToken::new();
    let prepared = PreparedInstall::prepare(&i, PRODUCT, &cancel).unwrap();
    let bytes = api
        .download(
            DownloadRequest {
                token: &refreshed.token,
                game: Game::Retail,
                region: Region::Eu,
                from: prepared.installed_version.as_deref(),
                manifest: &prepared.manifest,
            },
            &cancel,
            |_, _| {},
        )
        .await
        .unwrap();
    let license = api.license(&refreshed.token, Game::Retail).await.unwrap();
    let version = prepared
        .apply(&bytes, &license, Some("9.6.101"), &cancel, |_, _| {})
        .unwrap();
    assert_eq!(version, "9.6.101");
    let addon = i.addons().join(PRODUCT);
    assert_eq!(fs::read(addon.join("Guide.lua")).unwrap(), b"new guide");
    assert_eq!(
        fs::read(addon.join("Custom/MyGuide.lua")).unwrap(),
        b"my custom guide"
    );
    assert!(!addon.join("obsolete.lua").exists());
    assert_eq!(
        fs::read_to_string(addon.join("Licence.lua")).unwrap(),
        license
    );
    assert_eq!(
        fs::read(
            i.addons()
                .join(".openzygor-ZygorGuidesViewer/previous/Guide.lua")
        )
        .unwrap(),
        b"old guide"
    );
    assert_eq!(
        *calls.lock().unwrap(),
        ["validate", "validate", "list_packages", "comp", "license"]
    );
    server.abort();
}

#[test]
fn fresh_install_repair_and_uninstall_preserve_other_addons_and_wtf() {
    let tmp = tempfile::tempdir().unwrap();
    let i = game_folder(tmp.path());
    fs::create_dir_all(i.addons().join("OtherAddon")).unwrap();
    fs::write(i.addons().join("OtherAddon/file.lua"), b"other").unwrap();
    fs::create_dir_all(i.path.join("WTF")).unwrap();
    fs::write(i.path.join("WTF/Settings.lua"), b"settings").unwrap();
    let cancel = CancellationToken::new();
    let p = PreparedInstall::prepare(&i, PRODUCT, &cancel).unwrap();
    assert!(p.manifest.is_empty());
    p.apply(
        &full_archive("101"),
        "-- licence",
        Some("9.6.101"),
        &cancel,
        |_, _| {},
    )
    .unwrap();
    fs::write(i.addons().join(PRODUCT).join("Guide.lua"), b"damaged").unwrap();
    PreparedInstall::prepare(&i, PRODUCT, &cancel)
        .unwrap()
        .apply(
            &full_archive("101"),
            "-- licence",
            Some("9.6.101"),
            &cancel,
            |_, _| {},
        )
        .unwrap();
    assert_eq!(
        fs::read(i.addons().join(PRODUCT).join("Guide.lua")).unwrap(),
        b"new guide"
    );
    installer::uninstall(&i, PRODUCT).unwrap();
    assert!(!i.addons().join(PRODUCT).exists());
    assert!(
        i.addons()
            .join(".openzygor-ZygorGuidesViewer/uninstalled/Guide.lua")
            .exists()
    );
    // A subsequent install does not resurrect the backup as an interrupted update.
    let p = PreparedInstall::prepare(&i, PRODUCT, &cancel).unwrap();
    assert!(p.manifest.is_empty());
    assert_eq!(
        fs::read(i.addons().join("OtherAddon/file.lua")).unwrap(),
        b"other"
    );
    assert_eq!(
        fs::read(i.path.join("WTF/Settings.lua")).unwrap(),
        b"settings"
    );
}

#[test]
fn unsafe_archives_leave_current_installation_untouched() {
    for name in [
        "../escaped",
        "/tmp/escaped",
        "ZygorGuidesViewer/../../escaped",
        "ZygorGuidesViewer/../bad",
        "OtherAddon/file",
        "C:/escaped",
        "ZygorGuidesViewer/..\\..\\escaped",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let i = installed(tmp.path());
        let cancel = CancellationToken::new();
        let result = PreparedInstall::prepare(&i, PRODUCT, &cancel)
            .unwrap()
            .apply(
                &archive(&[(name, b"bad")]),
                "-- licence",
                None,
                &cancel,
                |_, _| {},
            );
        assert!(result.is_err(), "accepted {name}");
        assert_eq!(
            fs::read(i.addons().join(PRODUCT).join("Guide.lua")).unwrap(),
            b"old guide"
        );
        assert!(!i.addons().join("escaped").exists());
    }
}

#[test]
fn invalid_metadata_and_version_mismatches_do_not_touch_existing_guides() {
    for zip in [
        archive(&[(
            ".zygor-meta",
            br#"{"DELETE":["ZygorGuidesViewer/../../Wow.exe"]}"#,
        )]),
        archive(&[(".zygor-meta", br#"{"DELETE":["ZygorGuidesViewer"]}"#)]),
        archive(&[(".zygor-meta", b"not json")]),
        full_archive("102"),
        b"truncated".to_vec(),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let i = installed(tmp.path());
        let cancel = CancellationToken::new();
        assert!(
            PreparedInstall::prepare(&i, PRODUCT, &cancel)
                .unwrap()
                .apply(&zip, "-- licence", Some("9.6.101"), &cancel, |_, _| {})
                .is_err()
        );
        assert_eq!(
            installer::version(&i.addons().join(PRODUCT), PRODUCT).unwrap(),
            "9.6.100"
        );
        assert!(i.path.join("Wow.exe").exists());
    }
}

#[test]
fn cancellation_and_cross_process_lock_protect_the_installed_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let i = installed(tmp.path());
    let cancel = CancellationToken::new();
    let prepared = PreparedInstall::prepare(&i, PRODUCT, &cancel).unwrap();
    assert!(PreparedInstall::prepare(&i, PRODUCT, &cancel).is_err());
    let result = prepared.apply(&full_archive("101"), "-- licence", None, &cancel, |_, _| {
        cancel.cancel()
    });
    assert!(result.is_err());
    assert_eq!(
        installer::version(&i.addons().join(PRODUCT), PRODUCT).unwrap(),
        "9.6.100"
    );
    assert!(PreparedInstall::prepare(&i, PRODUCT, &CancellationToken::new()).is_ok());
}

#[test]
fn interrupted_commit_restores_the_previous_addon_before_hashing() {
    let tmp = tempfile::tempdir().unwrap();
    let i = installed(tmp.path());
    let work = i.addons().join(".openzygor-ZygorGuidesViewer");
    fs::create_dir_all(&work).unwrap();
    fs::rename(i.addons().join(PRODUCT), work.join("previous")).unwrap();
    let prepared = PreparedInstall::prepare(&i, PRODUCT, &CancellationToken::new()).unwrap();
    assert_eq!(prepared.installed_version.as_deref(), Some("9.6.100"));
    assert!(i.addons().join(PRODUCT).join("Guide.lua").is_file());
}

#[test]
fn edits_made_while_downloading_are_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let installation = installed(tmp.path());
    let cancel = CancellationToken::new();
    let prepared = PreparedInstall::prepare(&installation, PRODUCT, &cancel).unwrap();
    // The user saves a custom guide while the server prepares the update.
    let custom = installation
        .addons()
        .join(PRODUCT)
        .join("Custom/MyGuide.lua");
    fs::write(&custom, b"edited during download").unwrap();
    let result = prepared.apply(
        &full_archive("101"),
        "-- licence",
        Some("9.6.101"),
        &cancel,
        |_, _| {},
    );
    assert!(
        result.is_err(),
        "An installation must not overwrite concurrent edits"
    );
    assert_eq!(fs::read(custom).unwrap(), b"edited during download");
}

#[test]
fn metadata_with_no_deletions_is_a_valid_no_op_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let installation = installed(tmp.path());
    let cancel = CancellationToken::new();
    let bytes = archive(&[("ZygorGuidesViewer/.zygor-meta", br#"{"DELETE":null}"#)]);
    let result = PreparedInstall::prepare(&installation, PRODUCT, &cancel)
        .unwrap()
        .apply(
            &bytes,
            "-- renewed licence",
            Some("9.6.100"),
            &cancel,
            |_, _| {},
        );
    assert!(
        result.is_ok(),
        "An empty deletion list must be accepted: {result:?}"
    );
}

#[cfg(unix)]
#[test]
fn symlinks_in_existing_addons_and_archives_are_rejected() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let i = installed(tmp.path());
    let cancel = CancellationToken::new();
    symlink(tmp.path(), i.addons().join(PRODUCT).join("escape")).unwrap();
    assert!(PreparedInstall::prepare(&i, PRODUCT, &cancel).is_err());
    fs::remove_file(i.addons().join(PRODUCT).join("escape")).unwrap();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink(
        "ZygorGuidesViewer/escape",
        "../../",
        SimpleFileOptions::default(),
    )
    .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(
        PreparedInstall::prepare(&i, PRODUCT, &cancel)
            .unwrap()
            .apply(&bytes, "-- licence", None, &cancel, |_, _| {})
            .is_err()
    );
    let addons = i.addons();
    fs::rename(&addons, addons.with_file_name("real-addons")).unwrap();
    symlink(addons.with_file_name("real-addons"), &addons).unwrap();
    assert!(PreparedInstall::prepare(&i, PRODUCT, &cancel).is_err());
}

#[tokio::test]
async fn authentication_errors_never_include_credentials_or_request_urls() {
    let router = Router::new().route(
        "/",
        any(|| async {
            (
                StatusCode::UNAUTHORIZED,
                "secret-token should never be displayed",
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let api = Api::new(&format!("http://{address}/")).unwrap();
    let message = api
        .login("fixture-user", "secret-password")
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(message.contains("expired"));
    assert!(!message.contains("secret"));
    assert!(!message.contains("http"));
    server.abort();
    let message = api
        .validate("secret-token")
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(!message.contains("secret"));
    assert!(!message.contains("http"));
}

#[test]
fn periodic_update_preference_survives_existing_settings_and_save() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    assert!(!config::load(&path).unwrap().update_automatically);
    for enabled in [true, false] {
        fs::write(
            &path,
            serde_json::to_vec(&json!({"check_automatically": enabled})).unwrap(),
        )
        .unwrap();
        let mut settings = config::load(&path).unwrap();
        assert_eq!(settings.update_automatically, enabled);
        config::save(&path, &settings).unwrap();
        let serialized: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(serialized["update_automatically"], enabled);
        assert!(serialized.get("check_automatically").is_none());
        settings.update_automatically = !enabled;
        config::save(&path, &settings).unwrap();
        assert_eq!(config::load(&path).unwrap().update_automatically, !enabled);
    }
}

#[test]
fn configuration_and_folder_detection_use_linux_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let prefix = tmp.path().join(
        ".var/app/com.usebottles.bottles/data/bottles/bottles/Blizzard/drive_c/Program Files (x86)",
    );
    let installation = game_folder(&prefix);
    assert_eq!(discovery::discover(tmp.path()).len(), 1);
    assert!(discovery::validate(Game::ClassicProgression, &installation.path).is_err());
    let settings = Config {
        username: "fixture-user".into(),
        region: Region::Eu,
        installations: vec![installation],
        ..Default::default()
    };
    let path = tmp.path().join("config/settings.json");
    config::save(&path, &settings).unwrap();
    let serialized = fs::read_to_string(&path).unwrap();
    assert!(!serialized.contains("token"));
    assert!(!serialized.contains("password"));
    assert_eq!(config::load(&path).unwrap().installations.len(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
