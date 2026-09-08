//! 应用内更新（tauri-plugin-updater + GitHub Releases 静态 latest.json）。
//!
//! 设计要点（关键行为均已对插件源码核对，见 ATTRIBUTION.md 2026-09-05 更新调研节）：
//! - 检查与安装全部走 Rust 侧：前端只调用本模块的两个 command，
//!   便于把全部分支决策、外部调用结果与耗时写入 SAYALL_GATT_LOG 诊断日志。
//! - Windows 上 `download_and_install` 内部会 `std::process::exit(0)`（Drop 清理
//!   不会执行），因此 BLE 断开等成对清理必须注册在 `on_before_exit` 回调里，
//!   而不是依赖进程退出路径——2026-09-05"部署不得强杀"教训的更新版。
//! - 安装器以 passive（/P + /UPDATE + /R）运行：显示进度条、装完自动重启应用。
//! - 默认端点来自 tauri.conf.json（GitHub Releases stable latest.json）；用户
//!   显式开启预览版后，通过 GitHub Releases Atom feed 选择最高 SemVer 的已发布
//!   Release（包含 Pre-release）并在运行时覆盖为其 latest.json；环境变量
//!   `SAYALL_UPDATER_ENDPOINT` 可覆盖端点（release 构建强制 https，仅用于
//!   本地/开发验证，正式配置不含任何 dangerous 开关）。
//! - 超时：检查请求 30s；下载 20 分钟（安装器 ~10-20MB，慢速链路兜底）。

use crate::AppState;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Error as UpdaterError, UpdaterExt};

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const PROGRESS_EMIT_MIN_INTERVAL_BYTES: u64 = 128 * 1024;
const ENDPOINT_OVERRIDE_ENV: &str = "SAYALL_UPDATER_ENDPOINT";
const PREVIEW_RELEASES_FEED: &str =
    "https://github.com/GetSayAll/remote-mic-app-windows/releases.atom";
const PREVIEW_MANIFEST_NAME: &str = "latest.json";
const RELEASE_DOWNLOAD_PATH_PREFIX: &str = "/GetSayAll/remote-mic-app-windows/releases/download/";
/// 前端进度事件名（downloaded/contentLength/finished）。
const PROGRESS_EVENT: &str = "app-update-progress";

/// 端点无有效更新清单（如仓库尚无已发布 Release 导致 latest.json 404）时的
/// 呈现规则（2026-09-06 用户终裁）：与"服务器确认无新版本"一致，均提示
/// "已经是最新版本"，不让用户看到失败感文案；真实原因只写诊断日志。
/// 已知代价：正式 Release 若漏挂 latest.json 资产，用户也会看到"已经是
/// 最新版本"——由发布流程（windows-release.yml：缺 latest.json/.sig 即
/// fail-fast 拒绝建 Release）与诊断日志（check.fail error=...）兜底。
fn release_not_found_counts_as_up_to_date(error: &UpdaterError) -> bool {
    matches!(error, UpdaterError::ReleaseNotFound)
}

/// 用户可见的检查失败文案（普通用户不看技术细节；原始错误只进诊断日志）。
fn check_error_detail(error: &UpdaterError) -> String {
    match error {
        // 连接失败/超时/代理不可用。
        UpdaterError::Reqwest(_) => "网络连接失败，请稍后重试".to_owned(),
        _ => "检查暂时不可用，请稍后重试".to_owned(),
    }
}

/// 用户可见的下载/安装失败文案（同上，去技术化）。
fn install_error_message(error: &UpdaterError) -> String {
    match error {
        // 签名校验失败：安全相关，明确说"已停止安装"但不含术语。
        UpdaterError::Minisign(_) => "更新包校验失败，已停止安装".to_owned(),
        UpdaterError::Network(_) | UpdaterError::Reqwest(_) => {
            "下载更新失败，请检查网络后重试".to_owned()
        }
        _ => "下载或安装暂时失败，请稍后重试".to_owned(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdatePreferences {
    pub include_prereleases: bool,
}

#[derive(Debug, Deserialize)]
struct ReleasesFeed {
    #[serde(rename = "entry", default)]
    entries: Vec<ReleaseFeedEntry>,
}

#[derive(Debug, Deserialize)]
struct ReleaseFeedEntry {
    #[serde(rename = "link", default)]
    links: Vec<ReleaseFeedLink>,
}

#[derive(Debug, Deserialize)]
struct ReleaseFeedLink {
    #[serde(rename = "@rel")]
    rel: String,
    #[serde(rename = "@href")]
    href: String,
}

enum UpdateEndpoint {
    ConfiguredStable,
    Runtime(reqwest::Url),
    NoPublishedPreview,
}

#[derive(Debug)]
enum PreviewManifestError {
    InvalidReleaseUrl,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateProgress {
    downloaded: u64,
    content_length: Option<u64>,
    finished: bool,
}

/// 下载进度共享状态（on_chunk 与 on_download_finish 两个闭包共用，
/// 避免可变/不可变借用冲突）。
#[derive(Debug, Default)]
struct DownloadProgressState {
    downloaded: u64,
    content_length: Option<u64>,
    last_emitted: u64,
}

/// 功能点日志（AGENTS.md 规范）：与语音链路共用 SAYALL_GATT_LOG 载体，
/// `N <ms> len=  0 note=updater.<事件> <键值>` 结构化标记，不含设备身份/路径。
fn note(detail: String) {
    #[cfg(windows)]
    sayall_windows::gatt_note(format!("updater.{detail}"));
    #[cfg(not(windows))]
    let _ = detail;
}

fn elapsed_ms(started: Instant) -> u128 {
    started.elapsed().as_millis()
}

fn preview_manifest_endpoint(
    feed: &ReleasesFeed,
) -> Result<Option<(reqwest::Url, String)>, PreviewManifestError> {
    // 开关语义是“包含预览版”，不是“只看预览版”：在所有已发布的正式版
    // 与预览版中按 SemVer 取最高版本，避免较旧预览版遮住较新的正式版。
    // GitHub Releases Atom feed 不包含 Draft；tag 从公开的 alternate 链接读取。
    let Some((tag, _)) = feed
        .entries
        .iter()
        .filter_map(|entry| {
            let href = entry
                .links
                .iter()
                .find(|link| link.rel == "alternate")?
                .href
                .trim_end_matches('/');
            let tag = href.strip_prefix(
                "https://github.com/GetSayAll/remote-mic-app-windows/releases/tag/",
            )?;
            semver::Version::parse(tag.trim_start_matches('v'))
                .ok()
                .map(|version| (tag.to_owned(), version))
        })
        .max_by(|(_, left), (_, right)| left.cmp(right))
    else {
        return Ok(None);
    };
    let url = format!(
        "https://github.com/GetSayAll/remote-mic-app-windows/releases/download/{tag}/{PREVIEW_MANIFEST_NAME}"
    )
    .parse::<reqwest::Url>()
    .map_err(|_| PreviewManifestError::InvalidReleaseUrl)?;
    let trusted = url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.path().starts_with(RELEASE_DOWNLOAD_PATH_PREFIX);
    if !trusted {
        return Err(PreviewManifestError::InvalidReleaseUrl);
    }
    Ok(Some((url, tag)))
}

async fn resolve_update_endpoint(include_prereleases: bool) -> Result<UpdateEndpoint, String> {
    if let Some(endpoint) = std::env::var_os(ENDPOINT_OVERRIDE_ENV) {
        let endpoint = endpoint
            .to_string_lossy()
            .trim()
            .trim_matches('"')
            .to_owned();
        note(format!("check.endpoint_override url={endpoint}"));
        let url = endpoint.parse().map_err(|error| {
            note(format!(
                "check.fail stage=endpoint_override_parse error={error:?}"
            ));
            "更新配置异常，请联系开发者".to_owned()
        })?;
        return Ok(UpdateEndpoint::Runtime(url));
    }
    if !include_prereleases {
        return Ok(UpdateEndpoint::ConfiguredStable);
    }

    let started = Instant::now();
    note("check.preview_resolve_start".to_owned());
    // tauri-plugin-updater 在构建 Updater 时会安装相同的 ring provider；预览
    // 通道需先访问 GitHub feed，因此在构建 reqwest Client 前完成同一初始化。
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        note("check.fail channel=preview stage=tls_provider".to_owned());
        return Err("预览版更新暂时不可用，请稍后重试".to_owned());
    }
    let client = reqwest::Client::builder()
        .timeout(CHECK_TIMEOUT)
        .user_agent("SayAll-Windows-Updater")
        .build()
        .map_err(|error| {
            note(format!(
                "check.fail channel=preview stage=client_build error={error:?}"
            ));
            "预览版更新暂时不可用，请稍后重试".to_owned()
        })?;
    let response = client
        .get(PREVIEW_RELEASES_FEED)
        .header("Accept", "application/atom+xml")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| {
            note(format!(
                "check.fail channel=preview stage=releases_request error={error:?} took_ms={}",
                elapsed_ms(started)
            ));
            "网络连接失败，请稍后重试".to_owned()
        })?;
    let feed_xml = response.text().await.map_err(|error| {
        note(format!(
            "check.fail channel=preview stage=releases_read error={error:?} took_ms={}",
            elapsed_ms(started)
        ));
        "预览版更新暂时不可用，请稍后重试".to_owned()
    })?;
    let feed = quick_xml::de::from_str::<ReleasesFeed>(&feed_xml).map_err(|error| {
        note(format!(
            "check.fail channel=preview stage=releases_parse error={error:?} took_ms={}",
            elapsed_ms(started)
        ));
        "预览版更新暂时不可用，请稍后重试".to_owned()
    })?;
    let preview = preview_manifest_endpoint(&feed).map_err(|error| {
        note(format!(
            "check.fail channel=preview stage=manifest_url error={error:?}"
        ));
        "预览版更新暂时不可用，请稍后重试".to_owned()
    })?;
    match preview {
        Some((url, tag)) => {
            note(format!(
                "check.preview_resolved tag={tag} asset={PREVIEW_MANIFEST_NAME}"
            ));
            Ok(UpdateEndpoint::Runtime(url))
        }
        None => {
            note(format!(
                "check.preview_resolved available=false took_ms={}",
                elapsed_ms(started)
            ));
            Ok(UpdateEndpoint::NoPublishedPreview)
        }
    }
}

/// 构建更新器：注册安装前清理回调 + 端点覆盖 + 检查超时。
fn build_updater(
    app: &AppHandle,
    platform: Arc<dyn crate::platform::PlatformRuntime>,
    runtime_endpoint: Option<reqwest::Url>,
) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder().timeout(CHECK_TIMEOUT);
    // on_before_exit 在安装器启动前、std::process::exit(0) 前同步执行：
    // 显式断开 BLE 链路（正常断开序列 CCCD 退订/服务释放），避免残留
    // 未关闭的 GATT 会话把链路留成僵死状态。
    builder = builder.on_before_exit(move || {
        let started = Instant::now();
        let outcome = if platform.disconnect_remote().is_ok() {
            "ok"
        } else {
            "err"
        };
        note(format!(
            "install.before_exit disconnect={outcome} took_ms={}",
            elapsed_ms(started)
        ));
    });
    if let Some(endpoint) = runtime_endpoint {
        builder = builder.endpoints(vec![endpoint]).map_err(|error| {
            note(format!(
                "check.fail stage=runtime_endpoint_reject error={error:?}"
            ));
            "更新配置异常，请联系开发者".to_owned()
        })?;
    }
    builder.build().map_err(|error| {
        note(format!("check.fail stage=build error={error:?}"));
        "更新功能暂时不可用，请稍后重试".to_owned()
    })
}

#[tauri::command]
pub async fn check_app_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppUpdateInfo, String> {
    let started = Instant::now();
    let include_prereleases = state
        .settings
        .load()
        .map_err(|error| {
            note(format!("check.fail stage=preference_load error={error:?}"));
            "读取更新设置失败，请稍后重试".to_owned()
        })?
        .check_prerelease_updates;
    let channel = if include_prereleases {
        "preview"
    } else {
        "stable"
    };
    note(format!("check.start source=command channel={channel}"));
    let endpoint = resolve_update_endpoint(include_prereleases).await?;
    if matches!(&endpoint, UpdateEndpoint::NoPublishedPreview) {
        if let Ok(mut pending) = state.pending_update.lock() {
            *pending = None;
        }
        return Ok(AppUpdateInfo {
            current_version: app.package_info().version.to_string(),
            available: false,
            version: None,
            notes: None,
            date: None,
        });
    }
    let platform = Arc::clone(&state.platform);
    let runtime_endpoint = match endpoint {
        UpdateEndpoint::Runtime(url) => Some(url),
        UpdateEndpoint::ConfiguredStable | UpdateEndpoint::NoPublishedPreview => None,
    };
    let updater = build_updater(&app, platform, runtime_endpoint)?;
    match updater.check().await {
        Ok(Some(update)) => {
            let info = AppUpdateInfo {
                current_version: update.current_version.clone(),
                available: true,
                version: Some(update.version.clone()),
                notes: update.body.clone(),
                date: update.date.map(|date| date.to_string()),
            };
            note(format!(
                "check.ok available=true current={} latest={} took_ms={}",
                update.current_version,
                update.version,
                elapsed_ms(started)
            ));
            *state
                .pending_update
                .lock()
                .map_err(|_error| "更新状态异常，请重启应用后重试".to_owned())? = Some(update);
            Ok(info)
        }
        Ok(None) => {
            note(format!(
                "check.ok available=false took_ms={}",
                elapsed_ms(started)
            ));
            if let Ok(mut pending) = state.pending_update.lock() {
                *pending = None;
            }
            Ok(AppUpdateInfo {
                current_version: app.package_info().version.to_string(),
                available: false,
                version: None,
                notes: None,
                date: None,
            })
        }
        Err(error) => {
            // 日志保留插件原始错误（诊断用）；用户可见文案去技术化。
            note(format!(
                "check.fail error={error:?} took_ms={}",
                elapsed_ms(started)
            ));
            // 取不到清单（404 类）→ 呈现为"已经是最新版本"（2026-09-06 终裁）。
            if release_not_found_counts_as_up_to_date(&error) {
                note("check.presented_as_up_to_date reason=release_not_found".to_owned());
                if let Ok(mut pending) = state.pending_update.lock() {
                    *pending = None;
                }
                return Ok(AppUpdateInfo {
                    current_version: app.package_info().version.to_string(),
                    available: false,
                    version: None,
                    notes: None,
                    date: None,
                });
            }
            Err(check_error_detail(&error))
        }
    }
}

#[tauri::command]
pub fn get_app_update_preferences(
    state: State<'_, AppState>,
) -> Result<AppUpdatePreferences, String> {
    let include_prereleases = state
        .settings
        .load()
        .map_err(|error| {
            note(format!("preference.load result=failed error={error:?}"));
            "读取预览版更新设置失败，请稍后重试".to_owned()
        })?
        .check_prerelease_updates;
    note(format!(
        "preference.load include_prereleases={include_prereleases}"
    ));
    Ok(AppUpdatePreferences {
        include_prereleases,
    })
}

#[tauri::command]
pub async fn set_app_update_preferences(
    include_prereleases: bool,
    state: State<'_, AppState>,
) -> Result<AppUpdatePreferences, String> {
    let settings = state.settings.clone();
    let save_result = tauri::async_runtime::spawn_blocking(move || {
        settings.save_check_prerelease_updates(include_prereleases)
    })
    .await
    .map_err(|error| {
        note(format!(
            "preference.save result=task_failed include_prereleases={include_prereleases} error={error:?}"
        ));
        "保存预览版更新设置失败，请稍后重试".to_owned()
    })?;
    save_result.map_err(|error| {
        note(format!(
            "preference.save result=write_failed include_prereleases={include_prereleases} error={error:?}"
        ));
        "保存预览版更新设置失败，请稍后重试".to_owned()
    })?;
    note(format!(
        "preference.save result=ok include_prereleases={include_prereleases}"
    ));
    Ok(AppUpdatePreferences {
        include_prereleases,
    })
}

#[tauri::command]
pub async fn install_app_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let started = Instant::now();
    let mut update = state
        .pending_update
        .lock()
        .map_err(|error| {
            note(format!("install.fail stage=state_lock error={error:?}"));
            "更新状态异常，请重启应用后重试".to_owned()
        })?
        .take()
        .ok_or_else(|| {
            note("install.fail stage=no_pending_update".to_owned());
            "请先检查更新，再下载安装".to_owned()
        })?;
    note(format!("install.start version={}", update.version));
    // 下载整体超时（check 的超时不作用于下载请求）。
    update.timeout = Some(DOWNLOAD_TIMEOUT);

    let progress = Arc::new(Mutex::new(DownloadProgressState::default()));
    let chunk_progress = Arc::clone(&progress);
    let chunk_app = app.clone();
    let finish_progress = Arc::clone(&progress);
    let finish_app = app.clone();

    let result = update
        .download_and_install(
            move |chunk_length, total| {
                let Ok(mut progress) = chunk_progress.lock() else {
                    return;
                };
                progress.downloaded += chunk_length as u64;
                progress.content_length = total;
                // 限频上报：每 128KB 一次，避免 IPC 洪泛。
                if progress.downloaded - progress.last_emitted >= PROGRESS_EMIT_MIN_INTERVAL_BYTES {
                    progress.last_emitted = progress.downloaded;
                    let _ = chunk_app.emit(
                        PROGRESS_EVENT,
                        &AppUpdateProgress {
                            downloaded: progress.downloaded,
                            content_length: progress.content_length,
                            finished: false,
                        },
                    );
                }
            },
            move || {
                let snapshot = finish_progress.lock().map(|progress| AppUpdateProgress {
                    downloaded: progress.downloaded,
                    content_length: progress.content_length,
                    finished: true,
                });
                if let Ok(payload) = snapshot {
                    let _ = finish_app.emit(PROGRESS_EVENT, &payload);
                    note(format!(
                        "install.download_done bytes={} took_ms={}",
                        payload.downloaded,
                        elapsed_ms(started)
                    ));
                }
            },
        )
        .await;

    match result {
        Ok(()) => {
            // Windows 上安装成功时进程在 install 内部 exit(0)，此分支仅在
            // 非 Windows（本项目不涉及）或未来行为变化时到达。
            note("install.completed_after_return".to_owned());
            Ok(())
        }
        Err(error) => {
            let downloaded = progress
                .lock()
                .map(|progress| progress.downloaded)
                .unwrap_or(0);
            // 日志保留插件原始错误（诊断用）；用户可见文案去技术化。
            note(format!(
                "install.fail downloaded={downloaded} error={error:?} took_ms={}",
                elapsed_ms(started)
            ));
            Err(install_error_message(&error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(tags: &[&str]) -> ReleasesFeed {
        ReleasesFeed {
            entries: tags
                .iter()
                .map(|tag| ReleaseFeedEntry {
                    links: vec![ReleaseFeedLink {
                        rel: "alternate".to_owned(),
                        href: format!(
                            "https://github.com/GetSayAll/remote-mic-app-windows/releases/tag/{tag}"
                        ),
                    }],
                })
                .collect(),
        }
    }

    #[test]
    fn preview_channel_includes_prereleases_and_uses_highest_version() {
        let releases = feed(&["v0.2.1", "v0.2.2", "v0.2.0"]);
        let (endpoint, tag) = preview_manifest_endpoint(&releases).unwrap().unwrap();
        assert_eq!(tag, "v0.2.2");
        assert_eq!(
            endpoint.as_str(),
            "https://github.com/GetSayAll/remote-mic-app-windows/releases/download/v0.2.2/latest.json"
        );
    }

    #[test]
    fn preview_channel_ignores_foreign_and_non_semver_links() {
        let releases = ReleasesFeed {
            entries: vec![ReleaseFeedEntry {
                links: vec![
                    ReleaseFeedLink {
                        rel: "alternate".to_owned(),
                        href: "https://example.com/releases/tag/v9.9.9".to_owned(),
                    },
                    ReleaseFeedLink {
                        rel: "self".to_owned(),
                        href: "https://github.com/GetSayAll/remote-mic-app-windows/releases/tag/not-a-version"
                            .to_owned(),
                    },
                ],
            }],
        };
        assert!(preview_manifest_endpoint(&releases).unwrap().is_none());
    }

    #[test]
    fn preview_channel_reports_no_published_release() {
        let releases = feed(&[]);
        assert!(preview_manifest_endpoint(&releases).unwrap().is_none());
    }

    #[test]
    #[ignore = "访问 GitHub 公共 Releases Feed，仅在发布前显式运行"]
    fn live_preview_channel_resolves_a_signed_manifest_endpoint() {
        let endpoint = tauri::async_runtime::block_on(resolve_update_endpoint(true)).unwrap();
        let UpdateEndpoint::Runtime(url) = endpoint else {
            panic!("当前应存在已发布的预览版清单")
        };
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("github.com"));
        assert!(url.path().starts_with(RELEASE_DOWNLOAD_PATH_PREFIX));
        assert!(url.path().ends_with("/latest.json"));
    }

    /// 取不到清单（ReleaseNotFound，404 类）按"已经是最新版本"呈现；
    /// 其他错误（网络/IO 等）不享受该待遇，仍如实报错（2026-09-06 终裁）。
    #[test]
    fn release_not_found_is_presented_as_up_to_date() {
        assert!(release_not_found_counts_as_up_to_date(
            &UpdaterError::ReleaseNotFound
        ));
        assert!(!release_not_found_counts_as_up_to_date(
            &UpdaterError::Network("x".to_owned())
        ));
        assert!(!release_not_found_counts_as_up_to_date(
            &UpdaterError::EmptyEndpoints
        ));
    }

    /// 用户可见文案不得包含技术文本（原始错误只进诊断日志）。
    #[test]
    fn user_facing_error_copy_is_plain_chinese() {
        let network = UpdaterError::Network("download failed at byte 4096".to_owned());
        assert_eq!(
            install_error_message(&network),
            "下载更新失败，请检查网络后重试"
        );
        let io_check = UpdaterError::Io(std::io::Error::other("os error 123"));
        assert_eq!(check_error_detail(&io_check), "检查暂时不可用，请稍后重试");
        let io_install = UpdaterError::Io(std::io::Error::other("os error 123"));
        assert_eq!(
            install_error_message(&io_install),
            "下载或安装暂时失败，请稍后重试"
        );
        // 签名类错误的安全文案（minisign 错误不可直接构造，用 Network 分支旁证
        // 映射完整性由编译保证）。
        let fallback = UpdaterError::EmptyEndpoints;
        assert_eq!(
            install_error_message(&fallback),
            "下载或安装暂时失败，请稍后重试"
        );
    }

    /// Rust ↔ TypeScript JSON 契约（对齐仓库既有 PlatformSnapshot 契约夹具做法）。
    #[test]
    fn app_update_info_serializes_camel_case() {
        let info = AppUpdateInfo {
            current_version: "0.1.0".to_owned(),
            available: true,
            version: Some("0.2.0".to_owned()),
            notes: Some("修复若干问题".to_owned()),
            date: Some("2026-09-05T12:00:00Z".to_owned()),
        };
        let json = serde_json::to_string(&info).expect("序列化 AppUpdateInfo 失败");
        assert!(
            json.contains("\"currentVersion\":\"0.1.0\""),
            "字段应为 camelCase：{json}"
        );
        assert!(json.contains("\"available\":true"), "{json}");
        assert!(json.contains("\"version\":\"0.2.0\""), "{json}");
        assert!(json.contains("\"notes\":\"修复若干问题\""), "{json}");
        assert!(json.contains("\"date\":\"2026-09-05T12:00:00Z\""), "{json}");
    }

    #[test]
    fn app_update_progress_serializes_camel_case() {
        let progress = AppUpdateProgress {
            downloaded: 4096,
            content_length: Some(1_048_576),
            finished: false,
        };
        let json = serde_json::to_string(&progress).expect("序列化 AppUpdateProgress 失败");
        assert!(
            json.contains("\"downloaded\":4096") && json.contains("\"contentLength\":1048576"),
            "字段应为 camelCase：{json}"
        );
        assert!(json.contains("\"finished\":false"), "{json}");
    }

    /// 功能点日志必须真实落盘（AGENTS.md"一次日志拉取定位环节"规范；2026-09-05
    /// E2E 实证：release 构建拒绝 http 端点的早期失败路径漏打点导致日志无下文）。
    /// 通过 SAYALL_GATT_LOG 环境变量开 sink 后调用 note()，断言标记行写入文件。
    #[cfg(windows)]
    #[test]
    fn updater_notes_land_in_diagnostic_log() {
        let path = std::env::temp_dir().join(format!(
            "sayall-updater-note-test-{}.log",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        // 本测试二进制内无其他代码先初始化 gatt_sink（OnceLock 首次调用生效）。
        // SAFETY: 测试进程内单线程操作该环境变量，其余测试不读取它。
        unsafe { std::env::set_var("SAYALL_GATT_LOG", &path) };
        note("check.fail stage=endpoint_override_parse error=probe".to_owned());
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        unsafe { std::env::remove_var("SAYALL_GATT_LOG") };
        let _ = std::fs::remove_file(&path);
        assert!(
            contents.contains("note=updater.check.fail stage=endpoint_override_parse"),
            "updater 功能点日志未落盘：{contents}"
        );
    }
}
