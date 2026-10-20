//! 「推荐」页：只做官网页面的宿主，不复制推荐内容或推荐 UI。
//!
//! 官网 `/recommendations/` 设置了 `X-Frame-Options: DENY`，因此不能用 `<iframe>`；
//! 这里在 Tauri 主窗口上挂一个子 WebView（Windows 上即 WebView2 子视图），直接加载
//! 官网页面。布局、排序、筛选、详情和内容更新全部由官网负责，Windows 客户端只提供：
//!
//! - 与设置侧栏一致的固定内容区边界（侧栏 108px，其余全部给网页）；
//! - `embed=app` 嵌入模式、浅/深色主题和 Windows 平台标记；
//! - 加载完成前隐藏子 WebView；加载失败显示原生错误状态（重试 / 打开官网）；
//! - 官网内的推荐详情仍由网页自己的 `?item=` 路由处理；
//! - 站外链接、`target=_blank` 一律交给系统浏览器。
//!
//! 参考 Mac 侧产品规范 `docs/product/recommendations.md`（私有平台仓库
//! `9845c01`）与 `RemoteWebPageView` / `RecommendationsWebPage`（`725061f`）；
//! 未复制其代码。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewBuilder,
    WebviewUrl,
};
use tauri_plugin_opener::OpenerExt;

/// 子 WebView 的固定标签；同一时间只允许一个推荐页实例。
pub const LABEL: &str = "recommendations";
/// 用户指定：直接使用官网中文推荐页（英文页留作后续语言切换）。
const PAGE_URL: &str = "https://sayall.app/recommendations/";
const ENGLISH_PAGE_URL: &str = "https://sayall.app/en/recommendations/";
/// 与 `src/styles.css` 的 `.app-shell` 第一列保持一致。
const SIDEBAR_WIDTH: f64 = 108.0;
/// 首次创建时先放到窗口外，避免 about:blank 或加载中的页面闪一下。
const OFFSCREEN_X: f64 = -20_000.0;
const EVENT_NAME: &str = "recommendations-page-state";

/// 推荐页是官网页面，样式不在本仓：用初始化脚本注入与本仓 `src/styles.css`
/// 同款的滚动条隐藏规则（产品要求全应用统一不显示滚动条，只保留滚动能力）。
///
/// 用 `initialization_script`（WebView2 的 `AddScriptToExecuteOnDocumentCreated`）
/// 而不是加载完成后再 `eval`：脚本在每次文档创建时执行，站内跳转、换主题重导航
/// 都自动生效，也不会先闪一下滚动条。官网样式可能带更高特异性的
/// `::-webkit-scrollbar` 规则，所以这里用 `!important` 压过它。
const HIDE_SCROLLBARS_SCRIPT: &str = r#"
(() => {
  const CSS = [
    "*, *::before, *::after { scrollbar-width: none !important; -ms-overflow-style: none !important; }",
    "*::-webkit-scrollbar { width: 0 !important; height: 0 !important; display: none !important; }",
  ].join("\n");
  const apply = () => {
    const host = document.head || document.documentElement;
    if (!host) return false;
    if (document.getElementById("sayall-hide-scrollbars")) return true;
    const style = document.createElement("style");
    style.id = "sayall-hide-scrollbars";
    style.textContent = CSS;
    host.appendChild(style);
    return true;
  };
  if (!apply()) {
    document.addEventListener("DOMContentLoaded", apply, { once: true });
  }
})();
"#;

#[derive(Default)]
pub struct RecommendationsState {
    /// 每次进入页面 / 重试 / 换主题都递增；前端用它丢弃迟到回调。
    generation: AtomicU64,
    /// 当前允许内嵌的导航 ID（0 表示没有等待中的导航）。
    expected_navigation: AtomicU64,
    /// `expected_navigation` 属于哪一代。
    expected_generation: AtomicU64,
    /// 当前主文档的 HTTP 状态（0 表示还没收到）。WebView2 的
    /// `NavigationCompleted.IsSuccess` 对 4xx/5xx 仍为 true，必须单独判定。
    document_status: AtomicU64,
    /// 子 WebView 当前是否应可见；窗口缩放只在可见时调整边界。
    visible: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageStateEvent {
    generation: u64,
    status: &'static str,
    reason: Option<&'static str>,
}

fn log(message: String) {
    sayall_windows::gatt_note(format!("feature=recommendations {message}"));
}

fn about_blank() -> Url {
    Url::parse("about:blank").expect("about:blank 是固定合法地址")
}

fn page_url(theme: &str) -> Url {
    let theme = if theme == "dark" { "dark" } else { "light" };
    Url::parse(&format!(
        "{PAGE_URL}?embed=app&appPlatform=windows&theme={theme}"
    ))
    .expect("推荐页地址是固定合法地址")
}

/// 只允许官网推荐页在主视图内导航。外部链接由调用方转交系统浏览器。
fn is_allowed_navigation(url: &Url) -> bool {
    if url.as_str() == "about:blank" {
        return true;
    }
    if url.scheme() != "https" || url.host_str() != Some("sayall.app") {
        return false;
    }
    // 带用户信息/密码的地址不进入 App 内嵌视图。
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let path = url.path();
    ["/recommendations", "/en/recommendations"]
        .iter()
        .any(|base| path == *base || path.starts_with(&format!("{base}/")))
}

/// 「打开官网」的目标：保留当前详情参数，去掉 App 内嵌专用参数。
fn browser_destination(current: Option<&Url>) -> Url {
    let Some(current) = current else {
        return Url::parse(PAGE_URL).expect("推荐页地址是固定合法地址");
    };
    if current.as_str() == "about:blank" || !is_allowed_navigation(current) {
        return Url::parse(PAGE_URL).expect("推荐页地址是固定合法地址");
    }

    let english = current.path().starts_with("/en/");
    let mut target = Url::parse(if english { ENGLISH_PAGE_URL } else { PAGE_URL })
        .expect("推荐页地址是固定合法地址");
    target.set_path(current.path());
    target.set_fragment(current.fragment());

    let query: Vec<(String, String)> = current
        .query_pairs()
        .filter(|(key, _)| !matches!(key.as_ref(), "embed" | "appPlatform" | "theme"))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    if !query.is_empty() {
        target.query_pairs_mut().extend_pairs(query);
    }
    target
}

fn open_external(app: &AppHandle, url: &Url) {
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        log(format!(
            "event=navigation_blocked result=blocked reason=unsupported_url scheme={}",
            url.scheme()
        ));
        return;
    }
    match app.opener().open_url(url.as_str(), None::<&str>) {
        Ok(()) => log("event=external_link action=browser result=passed".to_owned()),
        Err(error) => log(format!(
            "event=external_link action=browser result=failed error={error}"
        )),
    }
}

/// 子 WebView 的边界：固定侧栏之外的整个客户区。
fn layout(app: &AppHandle) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let window = app.get_window("main")?;
    let scale = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some((
        LogicalPosition::new(SIDEBAR_WIDTH, 0.0),
        LogicalSize::new((size.width - SIDEBAR_WIDTH).max(1.0), size.height.max(1.0)),
    ))
}

/// 主窗口尺寸变化时保持侧栏宽度不变（`auto_resize` 按比例缩放会留下空隙）。
pub fn relayout(app: &AppHandle) {
    let state = app.state::<RecommendationsState>();
    if !state.visible.load(Ordering::Acquire) {
        return;
    }
    let Some(webview) = app.get_webview(LABEL) else {
        return;
    };
    if let Some((position, size)) = layout(app) {
        let _ = webview.set_position(position);
        let _ = webview.set_size(size);
    }
}

#[tauri::command]
pub async fn show_recommendations(app: AppHandle, theme: Option<String>) -> Result<u64, String> {
    let theme = theme.as_deref().unwrap_or("light");
    let target = page_url(theme);
    let state = app.state::<RecommendationsState>();
    let generation = state
        .generation
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    // 新请求开始后，旧导航的完成回调不再代表当前页面。
    state.expected_navigation.store(0, Ordering::Release);
    state.document_status.store(0, Ordering::Release);
    let (position, size) = layout(&app).ok_or_else(|| "无法读取主窗口尺寸".to_owned())?;

    if let Some(webview) = app.get_webview(LABEL) {
        webview
            .set_position(position)
            .map_err(|error| format!("无法调整推荐页面位置: {error}"))?;
        webview
            .set_size(size)
            .map_err(|error| format!("无法调整推荐页面大小: {error}"))?;
        webview
            .navigate(target)
            .map_err(|error| format!("无法加载推荐页面: {error}"))?;
        log(format!(
            "event=show phase=completed result=passed mode=reuse generation={generation} theme={theme}"
        ));
        return Ok(generation);
    }

    let window = app
        .get_window("main")
        .ok_or_else(|| "找不到主窗口".to_owned())?;
    let navigation_app = app.clone();
    let new_window_app = app.clone();
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(about_blank()))
        .initialization_script(HIDE_SCROLLBARS_SCRIPT)
        .on_navigation(move |url| {
            if is_allowed_navigation(url) {
                return true;
            }
            open_external(&navigation_app, url);
            false
        })
        .on_new_window(move |url, _features| {
            open_external(&new_window_app, &url);
            tauri::webview::NewWindowResponse::Deny
        });

    let webview = window
        .add_child(builder, LogicalPosition::new(OFFSCREEN_X, 0.0), size)
        .map_err(|error| format!("无法创建推荐页面容器: {error}"))?;
    webview
        .hide()
        .map_err(|error| format!("无法隐藏推荐页面容器: {error}"))?;
    attach_navigation_observers(&webview, app.clone())?;
    webview
        .navigate(target)
        .map_err(|error| format!("无法加载推荐页面: {error}"))?;
    log(format!(
        "event=show phase=completed result=passed mode=create generation={generation} theme={theme}"
    ));
    Ok(generation)
}

#[tauri::command]
pub fn hide_recommendations(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RecommendationsState>();
    state.visible.store(false, Ordering::Release);
    state.expected_navigation.store(0, Ordering::Release);
    // 让仍在路上的完成事件带旧 generation，前端会忽略。
    let generation = state
        .generation
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    if let Some(webview) = app.get_webview(LABEL) {
        webview
            .hide()
            .map_err(|error| format!("无法隐藏推荐页面: {error}"))?;
    }
    log(format!(
        "event=hide phase=completed result=passed generation={generation}"
    ));
    Ok(())
}

#[tauri::command]
pub fn open_recommendations_website(app: AppHandle) -> Result<(), String> {
    let current = app
        .get_webview(LABEL)
        .and_then(|webview| webview.url().ok());
    let target = browser_destination(current.as_ref());
    match app.opener().open_url(target.as_str(), None::<&str>) {
        Ok(()) => {
            log(format!(
                "event=open_website phase=completed result=passed detail={}",
                if target.query_pairs().any(|(key, _)| key == "item") {
                    "with_item"
                } else {
                    "list"
                }
            ));
            Ok(())
        }
        Err(error) => {
            log(format!(
                "event=open_website phase=completed result=failed error={error}"
            ));
            Err(format!("无法打开浏览器: {error}"))
        }
    }
}

/// 监听 WebView2 的导航开始/完成事件。
///
/// Tauri 的 `on_page_load` 只能区分 started/finished，无法区分 HTTP/网络失败；
/// WebView2 的 `NavigationCompleted.IsSuccess` 能给出真实结果，因此这里直接挂
/// COM 事件。事件只在主视图导航时触发（子 frame 用另一套事件），与产品规范一致。
#[cfg(windows)]
fn attach_navigation_observers(webview: &Webview, app: AppHandle) -> Result<(), String> {
    use webview2_com::{
        CoTaskMemPWSTR, Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_ERROR_STATUS,
        NavigationCompletedEventHandler, NavigationStartingEventHandler,
    };
    use windows_core_061::{BOOL, PWSTR};

    webview
        .with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else {
                log("event=observer phase=completed result=failed stage=controller".to_owned());
                return;
            };

            let starting_app = app.clone();
            let starting = NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let mut navigation_id = 0u64;
                if args.NavigationId(&mut navigation_id).is_err() {
                    return Ok(());
                }
                let mut uri = PWSTR::null();
                if args.Uri(&mut uri).is_err() {
                    return Ok(());
                }
                let uri = CoTaskMemPWSTR::from(uri).to_string();
                let allowed = Url::parse(&uri)
                    .map(|url| is_allowed_navigation(&url))
                    .unwrap_or(false);
                if allowed {
                    let state = starting_app.state::<RecommendationsState>();
                    state
                        .expected_navigation
                        .store(navigation_id, Ordering::Release);
                    state.expected_generation.store(
                        state.generation.load(Ordering::Acquire),
                        Ordering::Release,
                    );
                }
                Ok(())
            }));

            let completed_app = app.clone();
            let completed = NavigationCompletedEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let mut navigation_id = 0u64;
                if args.NavigationId(&mut navigation_id).is_err() {
                    return Ok(());
                }
                let state = completed_app.state::<RecommendationsState>();
                let expected = state.expected_navigation.swap(0, Ordering::AcqRel);
                if expected == 0 || expected != navigation_id {
                    return Ok(());
                }
                let generation = state.expected_generation.load(Ordering::Acquire);

                let mut success = BOOL(0);
                let _ = args.IsSuccess(&mut success);
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                let _ = args.WebErrorStatus(&mut status);
                // 主文档 HTTP 状态单独判定：WebView2 对 4xx/5xx 仍报 IsSuccess。
                let document_status = state.document_status.swap(0, Ordering::AcqRel);
                let http_ok = document_status == 0 || (200..400).contains(&document_status);

                if success.as_bool() && http_ok {
                    state.visible.store(true, Ordering::Release);
                    if let Some(webview) = completed_app.get_webview(LABEL) {
                        if let Some((position, size)) = layout(&completed_app) {
                            let _ = webview.set_position(position);
                            let _ = webview.set_size(size);
                        }
                        let _ = webview.show();
                    }
                    let _ = completed_app.emit(
                        EVENT_NAME,
                        PageStateEvent {
                            generation,
                            status: "ready",
                            reason: None,
                        },
                    );
                    log(format!(
                        "event=navigation phase=completed result=passed generation={generation} http_status={document_status}"
                    ));
                } else {
                    state.visible.store(false, Ordering::Release);
                    if let Some(webview) = completed_app.get_webview(LABEL) {
                        let _ = webview.hide();
                    }
                    let _ = completed_app.emit(
                        EVENT_NAME,
                        PageStateEvent {
                            generation,
                            status: "failed",
                            reason: Some(if http_ok {
                                "navigation_failed"
                            } else {
                                "http_error"
                            }),
                        },
                    );
                    log(format!(
                        "event=navigation phase=completed result=failed generation={generation} http_status={document_status} web_error_status={}",
                        status.0
                    ));
                }
                Ok(())
            }));

            let mut starting_token = 0;
            let starting_result = core.add_NavigationStarting(&starting, &mut starting_token);

            // 主文档响应状态：WebView2 对 4xx/5xx 仍报 `IsSuccess`，必须单独读
            // HTTP 状态。只认推荐页地址的主文档请求，避免把图片/接口的 4xx
            // 当成整页失败（产品规范：图片单项失败不算整页失败）。
            let response_app = app.clone();
            let response = webview2_com::WebResourceResponseReceivedEventHandler::create(
                Box::new(move |_sender, args| {
                    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
                    let Some(args) = args.as_ref() else {
                        return Ok(());
                    };
                    let Ok(request) = args.Request() else {
                        return Ok(());
                    };
                    let mut uri = PWSTR::null();
                    if request.Uri(&mut uri).is_err() {
                        return Ok(());
                    }
                    let uri = CoTaskMemPWSTR::from(uri).to_string();
                    let is_page = Url::parse(&uri)
                        .map(|url| is_allowed_navigation(&url))
                        .unwrap_or(false);
                    if !is_page {
                        return Ok(());
                    }
                    let Ok(response) = args.Response() else {
                        return Ok(());
                    };
                    let mut status = 0i32;
                    if response.StatusCode(&mut status).is_ok() {
                        response_app
                            .state::<RecommendationsState>()
                            .document_status
                            .store(status.max(0) as u64, Ordering::Release);
                    }
                    Ok(())
                }),
            );
            let mut response_token = 0;
            let response_result = {
                // `ICoreWebView2_2` 才提供该事件；旧运行时没有时跳过（不影响加载判定）。
                use windows_core_061::Interface as _;
                match core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2>()
                {
                    Ok(core2) => core2.add_WebResourceResponseReceived(&response, &mut response_token),
                    Err(_) => Ok(()),
                }
            };

            let mut completed_token = 0;
            let completed_result =
                core.add_NavigationCompleted(&completed, &mut completed_token);
            let result = starting_result.and(completed_result).and(response_result);
            log(format!(
                "event=observer phase=completed result={}",
                if result.is_ok() { "passed" } else { "failed" }
            ));
        })
        .map_err(|error| format!("无法监听推荐页面导航: {error}"))
}

#[cfg(not(windows))]
fn attach_navigation_observers(_webview: &Webview, _app: AppHandle) -> Result<(), String> {
    Err("推荐页面仅在 Windows 上提供".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_page_url_carries_app_mode_and_theme() {
        let light = page_url("light");
        assert_eq!(
            light.as_str(),
            "https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=light"
        );
        let dark = page_url("dark");
        assert!(dark.as_str().ends_with("theme=dark"));
        // 未知主题按浅色处理，不把任意字符串写进 URL。
        assert!(page_url("system").as_str().ends_with("theme=light"));
    }

    #[test]
    fn only_official_recommendation_paths_are_embedded() {
        for allowed in [
            "https://sayall.app/recommendations/",
            "https://sayall.app/recommendations/?item=vokie",
            "https://sayall.app/en/recommendations/",
            "https://sayall.app/en/recommendations/?item=xiaomi-2",
            "about:blank",
        ] {
            assert!(
                is_allowed_navigation(&Url::parse(allowed).unwrap()),
                "{allowed}"
            );
        }
        for blocked in [
            "https://sayall.app/",
            "https://sayall.app/market/",
            "https://example.com/recommendations/",
            "http://sayall.app/recommendations/",
            "https://user@sayall.app/recommendations/",
        ] {
            assert!(
                !is_allowed_navigation(&Url::parse(blocked).unwrap()),
                "{blocked}"
            );
        }
    }

    #[test]
    fn browser_destination_keeps_item_and_drops_embed_parameters() {
        let current = Url::parse(
            "https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=dark&item=vokie",
        )
        .unwrap();
        let target = browser_destination(Some(&current));
        assert_eq!(
            target.as_str(),
            "https://sayall.app/recommendations/?item=vokie"
        );

        let english = Url::parse(
            "https://sayall.app/en/recommendations/?embed=app&theme=light&item=xiaomi-2",
        )
        .unwrap();
        assert_eq!(
            browser_destination(Some(&english)).as_str(),
            "https://sayall.app/en/recommendations/?item=xiaomi-2"
        );
    }

    #[test]
    fn browser_destination_falls_back_to_chinese_list_for_unrelated_pages() {
        let other = Url::parse("https://sayall.app/market/").unwrap();
        assert_eq!(
            browser_destination(Some(&other)).as_str(),
            "https://sayall.app/recommendations/"
        );
        assert_eq!(
            browser_destination(None).as_str(),
            "https://sayall.app/recommendations/"
        );
        let blank = Url::parse("about:blank").unwrap();
        assert_eq!(
            browser_destination(Some(&blank)).as_str(),
            "https://sayall.app/recommendations/"
        );
    }

    /// 滚动条隐藏必须同时覆盖三点：通用属性、webkit 伪元素、以及官网可能存在的
    /// 更高特异性规则（`!important`）。少了任何一条，推荐页会重新冒出滚动条。
    #[test]
    fn hide_scrollbars_script_covers_standard_and_webkit_rules() {
        for fragment in [
            "scrollbar-width: none !important",
            "-ms-overflow-style: none !important",
            "*::-webkit-scrollbar",
            "width: 0 !important",
            "display: none !important",
        ] {
            assert!(
                HIDE_SCROLLBARS_SCRIPT.contains(fragment),
                "注入脚本缺少「{fragment}」，推荐页滚动条会重新出现"
            );
        }
        // 文档创建时 head 可能还没解析出来，必须留 DOMContentLoaded 兜底。
        assert!(HIDE_SCROLLBARS_SCRIPT.contains("DOMContentLoaded"));
        // 同一文档重复注入只应留下一个 style，避免站内跳转累积节点。
        assert!(HIDE_SCROLLBARS_SCRIPT.contains("sayall-hide-scrollbars"));
    }

    /// 推荐页子 WebView 必须挂上这个脚本；漏挂的话整页滚动条直接回来。
    ///
    /// 拼字符串是为了让断言不在源码里留下完整字面量——否则测试能自己满足自己，
    /// 删掉挂载点也不会变红（阳性对照实证）。
    #[test]
    fn recommendations_webview_injects_hide_scrollbars_script() {
        let needle = concat!(".initialization_script(", "HIDE_SCROLLBARS_SCRIPT)");
        assert!(
            include_str!("recommendations.rs").contains(needle),
            "推荐页子 WebView 没有注入滚动条隐藏脚本"
        );
    }

    /// 本仓页面与官网页面的隐藏规则要同源：`src/styles.css` 一旦被改回可见滚动条，
    /// 两个入口（主界面 / 推荐页）就只剩一半生效，这里钉住两边关键声明一致。
    #[test]
    fn stylesheet_keeps_matching_scrollbar_hiding_rules() {
        let stylesheet = include_str!("../../src/styles.css");
        for fragment in [
            "scrollbar-width: none",
            "-ms-overflow-style: none",
            "::-webkit-scrollbar",
            "display: none",
        ] {
            assert!(
                stylesheet.contains(fragment),
                "src/styles.css 缺少「{fragment}」，与推荐页注入规则不再同源"
            );
        }
    }
}
