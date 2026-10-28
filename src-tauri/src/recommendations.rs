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

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;

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

/// 页面阶段：与 Mac 的 `RemoteWebPagePhase` 对齐。
const PHASE_IDLE: u8 = 0;
const PHASE_LOADING: u8 = 1;
const PHASE_READY: u8 = 2;
const PHASE_FAILED: u8 = 3;

fn phase_label(phase: u8) -> &'static str {
    match phase {
        PHASE_LOADING => "loading",
        PHASE_READY => "ready",
        PHASE_FAILED => "failed",
        PHASE_IDLE => "idle",
        _ => "unknown",
    }
}

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
    /// 当前页面阶段；只有 `READY` 且主题一致时才保留已就绪文档。
    phase: AtomicU8,
    /// 当前页面能否后退；决定顶部「返回上一页」条是否占位。
    can_go_back: AtomicBool,
    /// 导航超时看门狗令牌；每次新导航/离开递增，让旧定时器失效。
    nav_timeout_token: AtomicU64,
    /// 当前页面使用的主题；切主题必须重新导航。
    theme: Mutex<String>,
    /// 当前页面使用的语言（`zh-Hans` / `en`）；切语言必须重新导航。
    language: Mutex<String>,
    /// 本次 App 发起的导航成功后是否清空 WebView 历史。
    ///
    /// 重试/应用内换主题或语言/首次加载不是「用户浏览过的上一页」，不应成为
    /// 「返回上一页」目标；失败页或旧主题页留在历史里会让顶部凭空出现返回条。
    reset_history_after_navigation: AtomicBool,
    /// 正在等待 `Page.resetNavigationHistory` 回调；期间一律按不可后退处理，
    /// 防止重置完成前的 HistoryChanged 把返回条闪出来。
    history_reset_pending: AtomicBool,
}

/// 进入推荐页时对现有文档的处理方式。
#[derive(Debug, PartialEq, Eq)]
enum ShowIntent {
    /// 重新导航（首次、失败后按普通入口进入、切主题/语言、重试）。
    Navigate,
    /// 已就绪且主题与语言一致：直接显示，保留筛选/详情/滚动位置。
    RetainedReady,
    /// 已失败且主题与语言一致：继续显示失败页，等待用户点「重试」。
    RetainedFailed,
}

fn show_intent(phase: u8, same_appearance: bool, retry: bool) -> ShowIntent {
    if retry || !same_appearance {
        return ShowIntent::Navigate;
    }
    match phase {
        PHASE_READY => ShowIntent::RetainedReady,
        PHASE_FAILED => ShowIntent::RetainedFailed,
        _ => ShowIntent::Navigate,
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageStateEvent {
    generation: u64,
    status: &'static str,
    reason: Option<&'static str>,
    can_go_back: Option<bool>,
}

fn log(message: String) {
    sayall_windows::gatt_note(format!("feature=recommendations {message}"));
}

fn emit_ready(app: &AppHandle, generation: u64) {
    let state = app.state::<RecommendationsState>();
    let _ = app.emit(
        EVENT_NAME,
        PageStateEvent {
            generation,
            status: "ready",
            reason: None,
            can_go_back: Some(state.can_go_back.load(Ordering::Acquire)),
        },
    );
}

fn emit_failed(app: &AppHandle, generation: u64, reason: &'static str) {
    let _ = app.emit(
        EVENT_NAME,
        PageStateEvent {
            generation,
            status: "failed",
            reason: Some(reason),
            can_go_back: None,
        },
    );
}

/// 主文档导航超时；与 Mac 的 `RemoteWebPageConfiguration.timeout = 20` 对齐。
#[cfg(windows)]
const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(20);

/// 为每次允许的导航启动看门狗。
///
/// 前端超时只覆盖容器主动发起的 show/retry；网页自己触发的整页导航（例如站点内的
/// 语言切换、重新加载）不会经过前端的计时器。Mac 容器在每次导航开始时都启动超时，
/// 这里补齐同样的覆盖：20 秒内没有完成就停止导航并显示失败页。
#[cfg(windows)]
fn arm_navigation_timeout(app: &AppHandle, navigation_id: u64) {
    let state = app.state::<RecommendationsState>();
    let token = state.nav_timeout_token.fetch_add(1, Ordering::AcqRel) + 1;
    let timeout_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sayall-recommendations-timeout".to_owned())
        .spawn(move || {
            std::thread::sleep(NAVIGATION_TIMEOUT);
            let state = timeout_app.state::<RecommendationsState>();
            if state.nav_timeout_token.load(Ordering::Acquire) != token {
                return;
            }
            if state.phase.load(Ordering::Acquire) != PHASE_LOADING {
                return;
            }
            if state.expected_navigation.load(Ordering::Acquire) != navigation_id {
                return;
            }
            state.expected_navigation.store(0, Ordering::Release);
            state
                .reset_history_after_navigation
                .store(false, Ordering::Release);
            state.phase.store(PHASE_FAILED, Ordering::Release);
            state.visible.store(false, Ordering::Release);
            if let Some(webview) = timeout_app.get_webview(LABEL) {
                let _ = webview.with_webview(|platform| unsafe {
                    if let Ok(core) = platform.controller().CoreWebView2() {
                        let _ = core.Stop();
                    }
                });
                let _ = webview.hide();
            }
            let generation = state.generation.load(Ordering::Acquire);
            emit_failed(&timeout_app, generation, "timeout");
            log(format!(
                "event=navigation phase=completed result=failed reason=timeout nav_id={navigation_id} generation={generation}"
            ));
        });
    if let Err(error) = spawned {
        log(format!(
            "event=navigation phase=completed result=failed reason=timeout_spawn_failed error={error}"
        ));
    }
}

fn about_blank() -> Url {
    Url::parse("about:blank").expect("about:blank 是固定合法地址")
}

/// 推荐站只支持简体中文与英文；任何非 `en` 值都按中文处理（与 Mac
/// `language.hasPrefix("en")` 同语义）。
fn normalized_language(language: &str) -> &'static str {
    if language.to_ascii_lowercase().starts_with("en") {
        "en"
    } else {
        "zh-Hans"
    }
}

fn page_url(theme: &str, language: &str) -> Url {
    let theme = if theme == "dark" { "dark" } else { "light" };
    let base = if normalized_language(language) == "en" {
        ENGLISH_PAGE_URL
    } else {
        PAGE_URL
    };
    Url::parse(&format!(
        "{base}?embed=app&appPlatform=windows&theme={theme}"
    ))
    .expect("推荐页地址是固定合法地址")
}

/// 当前推荐页路径中，语言前缀之后的后缀（含前导 `/`）。
fn recommendation_suffix(path: &str) -> Option<&str> {
    path.strip_prefix("/en/recommendations")
        .or_else(|| path.strip_prefix("/recommendations"))
}

/// 重新加载当前页面（保留 `item` 等站内参数），套用新的 App 主题与语言。
///
/// Mac 的容器在重试/切外观时重载 `state.currentURL`；这里同理，避免详情页
/// 失败后重试把用户丢回列表。地址不合法或仍在 `about:blank` 时回落到列表页。
fn embedded_destination(current: Option<&Url>, theme: &str, language: &str) -> Url {
    let language = normalized_language(language);
    let Some(current) =
        current.filter(|url| url.as_str() != "about:blank" && is_allowed_navigation(url))
    else {
        return page_url(theme, language);
    };

    let base = if language == "en" {
        ENGLISH_PAGE_URL
    } else {
        PAGE_URL
    };
    let mut target = Url::parse(base).expect("推荐页地址是固定合法地址");
    if let Some(suffix) = recommendation_suffix(current.path()) {
        if !suffix.is_empty() && suffix != "/" {
            let joined = format!("{}{}", target.path(), suffix.trim_start_matches('/'));
            target.set_path(&joined);
        }
    }
    target.set_fragment(current.fragment());

    let query: Vec<(String, String)> = current
        .query_pairs()
        .filter(|(key, _)| !matches!(key.as_ref(), "embed" | "appPlatform" | "theme"))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    target
        .query_pairs_mut()
        .append_pair("embed", "app")
        .append_pair("appPlatform", "windows")
        .append_pair("theme", if theme == "dark" { "dark" } else { "light" });
    if !query.is_empty() {
        target.query_pairs_mut().extend_pairs(query);
    }
    target
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
fn browser_destination(current: Option<&Url>, language: &str) -> Url {
    let fallback = if normalized_language(language) == "en" {
        ENGLISH_PAGE_URL
    } else {
        PAGE_URL
    };
    let Some(current) = current else {
        return Url::parse(fallback).expect("推荐页地址是固定合法地址");
    };
    if current.as_str() == "about:blank" || !is_allowed_navigation(current) {
        return Url::parse(fallback).expect("推荐页地址是固定合法地址");
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

/// 顶部「返回上一页」条的高度（与 Mac 的 44pt 行高一致）。
const BACK_BAR_HEIGHT: f64 = 44.0;

fn back_bar_offset(state: &RecommendationsState) -> f64 {
    if state.phase.load(Ordering::Acquire) == PHASE_READY
        && state.can_go_back.load(Ordering::Acquire)
    {
        BACK_BAR_HEIGHT
    } else {
        0.0
    }
}

/// 子 WebView 的边界：固定侧栏之外、顶部返回条之下的整个客户区。
fn layout_with_top(app: &AppHandle, top: f64) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let window = app.get_window("main")?;
    let scale = window.scale_factor().ok()?;
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);
    Some((
        LogicalPosition::new(SIDEBAR_WIDTH, top),
        LogicalSize::new(
            (size.width - SIDEBAR_WIDTH).max(1.0),
            (size.height - top).max(1.0),
        ),
    ))
}

fn layout(app: &AppHandle) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let state = app.state::<RecommendationsState>();
    layout_with_top(app, back_bar_offset(&state))
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
pub async fn show_recommendations(
    app: AppHandle,
    theme: Option<String>,
    language: Option<String>,
) -> Result<u64, String> {
    present(
        app,
        theme.as_deref().unwrap_or("light"),
        language.as_deref().unwrap_or("zh-Hans"),
        false,
    )
    .await
}

/// 重试：重新加载当前页面（保留 `?item=` 详情参数），与 Mac 容器一致。
#[tauri::command]
pub async fn retry_recommendations(
    app: AppHandle,
    theme: Option<String>,
    language: Option<String>,
) -> Result<u64, String> {
    present(
        app,
        theme.as_deref().unwrap_or("light"),
        language.as_deref().unwrap_or("zh-Hans"),
        true,
    )
    .await
}

async fn present(app: AppHandle, theme: &str, language: &str, retry: bool) -> Result<u64, String> {
    let theme = if theme == "dark" { "dark" } else { "light" };
    let language = normalized_language(language);
    let state = app.state::<RecommendationsState>();
    let generation = state
        .generation
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    // 新请求开始后，旧导航的完成回调不再代表当前页面。
    state.expected_navigation.store(0, Ordering::Release);
    state.document_status.store(0, Ordering::Release);
    state.nav_timeout_token.fetch_add(1, Ordering::AcqRel);
    state
        .reset_history_after_navigation
        .store(false, Ordering::Release);
    // 新导航从顶部开始；只有已就绪且可后退时才给「返回上一页」条留位。
    let (position, size) =
        layout_with_top(&app, 0.0).ok_or_else(|| "无法读取主窗口尺寸".to_owned())?;

    if let Some(webview) = app.get_webview(LABEL) {
        webview
            .set_position(position)
            .map_err(|error| format!("无法调整推荐页面位置: {error}"))?;
        webview
            .set_size(size)
            .map_err(|error| format!("无法调整推荐页面大小: {error}"))?;

        let same_theme = state
            .theme
            .lock()
            .map(|value| value.as_str() == theme)
            .unwrap_or(false);
        let same_language = state
            .language
            .lock()
            .map(|value| value.as_str() == language)
            .unwrap_or(false);
        let phase = state.phase.load(Ordering::Acquire);
        match show_intent(phase, same_theme && same_language, retry) {
            ShowIntent::RetainedReady => {
                state.visible.store(true, Ordering::Release);
                if let Some((position, size)) = layout(&app) {
                    let _ = webview.set_position(position);
                    let _ = webview.set_size(size);
                }
                webview
                    .show()
                    .map_err(|error| format!("无法显示推荐页面: {error}"))?;
                emit_ready(&app, generation);
                log(format!(
                    "event=show phase=completed result=passed mode=retained phase=ready generation={generation} theme={theme} language={language} can_go_back={}",
                    state.can_go_back.load(Ordering::Acquire)
                ));
                return Ok(generation);
            }
            ShowIntent::RetainedFailed => {
                state.visible.store(false, Ordering::Release);
                let _ = webview.hide();
                emit_failed(&app, generation, "retained_failure");
                log(format!(
                    "event=show phase=completed result=passed mode=retained phase=failed generation={generation} theme={theme} language={language}"
                ));
                return Ok(generation);
            }
            ShowIntent::Navigate => {}
        }

        let current = webview.url().ok();
        let target = embedded_destination(current.as_ref(), theme, language);
        if let Ok(mut value) = state.theme.lock() {
            *value = theme.to_owned();
        }
        if let Ok(mut value) = state.language.lock() {
            *value = language.to_owned();
        }
        state.phase.store(PHASE_LOADING, Ordering::Release);
        // 主题/语言切换是「同一页换一套外观或语言」，不是一次新的浏览。用
        // `location.replace` 替换当前历史条目，否则旧页面会留在后退栈里，
        // 深色模式下刚打开/刚切换就会出现不该有的「返回上一页」条。
        // 只对已就绪文档这样做：加载中的文档执行脚本可能被导航丢弃。
        let replace_mode = match (same_theme, same_language) {
            (false, false) => "appearance_replace",
            (false, true) => "theme_replace",
            (true, false) => "language_replace",
            (true, true) => "reuse",
        };
        let mode = if retry {
            state
                .reset_history_after_navigation
                .store(true, Ordering::Release);
            webview
                .navigate(target)
                .map_err(|error| format!("无法加载推荐页面: {error}"))?;
            "retry"
        } else if (!same_theme || !same_language) && phase == PHASE_READY {
            let script = format!(
                "window.location.replace({});",
                serde_json::to_string(&target)
                    .map_err(|error| format!("无法编码推荐页地址: {error}"))?
            );
            match webview.eval(&script) {
                Ok(()) => replace_mode,
                Err(error) => {
                    log(format!(
                        "event=appearance_reload phase=completed result=failed reason=eval_failed error={error}"
                    ));
                    webview
                        .navigate(target)
                        .map_err(|error| format!("无法加载推荐页面: {error}"))?;
                    "reuse"
                }
            }
        } else {
            state
                .reset_history_after_navigation
                .store(true, Ordering::Release);
            webview
                .navigate(target)
                .map_err(|error| format!("无法加载推荐页面: {error}"))?;
            "reuse"
        };
        log(format!(
            "event=show phase=completed result=passed mode={mode} generation={generation} theme={theme} language={language}"
        ));
        return Ok(generation);
    }

    let window = app
        .get_window("main")
        .ok_or_else(|| "找不到主窗口".to_owned())?;
    let new_window_app = app.clone();
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(about_blank()))
        .initialization_script(HIDE_SCROLLBARS_SCRIPT)
        // 主视图只允许推荐页；站外链接由 NavigationStarting 里的用户手势判定
        // 交给系统浏览器（见 attach_navigation_observers），这里不再自动开浏览器。
        .on_navigation(|url| is_allowed_navigation(url))
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
    if let Ok(mut value) = state.theme.lock() {
        *value = theme.to_owned();
    }
    if let Ok(mut value) = state.language.lock() {
        *value = language.to_owned();
    }
    state.phase.store(PHASE_LOADING, Ordering::Release);
    attach_navigation_observers(&webview, app.clone())?;
    let target = if retry {
        embedded_destination(None, theme, language)
    } else {
        page_url(theme, language)
    };
    state
        .reset_history_after_navigation
        .store(true, Ordering::Release);
    webview
        .navigate(target)
        .map_err(|error| format!("无法加载推荐页面: {error}"))?;
    log(format!(
        "event=show phase=completed result=passed mode=create generation={generation} theme={theme} language={language}"
    ));
    Ok(generation)
}

#[tauri::command]
pub fn hide_recommendations(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RecommendationsState>();
    state.visible.store(false, Ordering::Release);
    state.expected_navigation.store(0, Ordering::Release);
    state.nav_timeout_token.fetch_add(1, Ordering::AcqRel);
    state
        .reset_history_after_navigation
        .store(false, Ordering::Release);
    // 让仍在路上的完成事件带旧 generation，前端会忽略。
    let generation = state
        .generation
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let phase = state.phase.load(Ordering::Acquire);
    let mut stopped = false;
    if let Some(webview) = app.get_webview(LABEL) {
        webview
            .hide()
            .map_err(|error| format!("无法隐藏推荐页面: {error}"))?;
        // 离开页面时停止本次加载，避免挂起的导航在后台继续占用网络/渲染资源。
        if phase == PHASE_LOADING {
            stopped = true;
            let _ = webview.with_webview(|platform| unsafe {
                if let Ok(core) = platform.controller().CoreWebView2() {
                    let _ = core.Stop();
                }
            });
        }
    }
    log(format!(
        "event=hide phase=completed result=passed generation={generation} phase={} stopped={stopped}",
        phase_label(phase)
    ));
    Ok(())
}

#[tauri::command]
pub fn open_recommendations_website(app: AppHandle) -> Result<(), String> {
    let current = app
        .get_webview(LABEL)
        .and_then(|webview| webview.url().ok());
    let language = app
        .state::<RecommendationsState>()
        .language
        .lock()
        .map(|value| value.clone())
        .unwrap_or_else(|_| "zh-Hans".to_owned());
    let target = browser_destination(current.as_ref(), &language);
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

/// 顶部「返回上一页」：让子 WebView 后退；历史变化事件会更新占位与前端状态。
#[tauri::command]
pub fn go_back_recommendations(app: AppHandle) -> Result<(), String> {
    let Some(webview) = app.get_webview(LABEL) else {
        return Ok(());
    };
    webview
        .with_webview(|platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else {
                log("event=go_back phase=completed result=failed reason=controller".to_owned());
                return;
            };
            match core.GoBack() {
                Ok(()) => log("event=go_back phase=completed result=passed".to_owned()),
                Err(error) => log(format!(
                    "event=go_back phase=completed result=failed error={error}"
                )),
            }
        })
        .map_err(|error| format!("无法返回推荐页上一页: {error}"))
}

/// 判断一次导航完成事件是否属于当前等待中的导航；匹配时才消费等待中的 ID。
///
/// 不能像旧实现那样先 `swap(0)` 再比对：任何一次**不匹配**的完成事件（取消的
/// `about:blank`、上一代页面迟到的完成、站内跳转被新导航取代）都会把当前
/// 等待中的 ID 清掉，随后真正的完成事件被当成迟到事件忽略，推荐页永远停在
/// 「正在加载网页…」。2026-10-24 实证：首次加载超过两分钟，用户离开后重进，
/// 旧导航的完成事件先到并吞掉了新导航的等待 ID。
fn consume_matching_navigation(state: &RecommendationsState, navigation_id: u64) -> bool {
    let expected = state.expected_navigation.load(Ordering::Acquire);
    expected != 0
        && expected == navigation_id
        && state
            .expected_navigation
            .compare_exchange(expected, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
}

/// 监听 WebView2 的导航开始/完成事件。
///
/// Tauri 的 `on_page_load` 只能区分 started/finished，无法区分 HTTP/网络失败；
/// WebView2 的 `NavigationCompleted.IsSuccess` 能给出真实结果，因此这里直接挂
/// COM 事件。事件只在主视图导航时触发（子 frame 用另一套事件），与产品规范一致。
#[cfg(windows)]
fn attach_navigation_observers(webview: &Webview, app: AppHandle) -> Result<(), String> {
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, CoTaskMemPWSTR, ExecuteScriptCompletedHandler,
        HistoryChangedEventHandler,
        Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
            COREWEBVIEW2_WEB_ERROR_STATUS,
        },
        NavigationCompletedEventHandler, NavigationStartingEventHandler, ProcessFailedEventHandler,
    };
    use windows_core_061::{BOOL, PCWSTR, PWSTR};

    webview
        .with_webview(move |platform| unsafe {
            let Ok(core) = platform.controller().CoreWebView2() else {
                log("event=observer phase=completed result=failed stage=controller".to_owned());
                return;
            };

            let starting_app = app.clone();
            let starting = NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    log("event=navigation phase=started result=failed reason=missing_args".to_owned());
                    return Ok(());
                };
                let mut navigation_id = 0u64;
                if args.NavigationId(&mut navigation_id).is_err() {
                    log("event=navigation phase=started result=failed reason=navigation_id".to_owned());
                    return Ok(());
                }
                let mut uri = PWSTR::null();
                if args.Uri(&mut uri).is_err() {
                    log(format!(
                        "event=navigation phase=started result=failed reason=uri nav_id={navigation_id}"
                    ));
                    return Ok(());
                }
                let uri = CoTaskMemPWSTR::from(uri).to_string();
                let parsed = Url::parse(&uri);
                let allowed = parsed
                    .as_ref()
                    .map(|url| is_allowed_navigation(url))
                    .unwrap_or(false);
                let path = parsed
                    .as_ref()
                    .map(|url| url.path().to_owned())
                    .unwrap_or_else(|_| "unparsed".to_owned());
                let mut user_initiated = BOOL(0);
                let user_initiated_read = args.IsUserInitiated(&mut user_initiated).is_ok();
                let mut redirected = BOOL(0);
                let _ = args.IsRedirected(&mut redirected);
                let state = starting_app.state::<RecommendationsState>();
                if allowed {
                    state
                        .expected_navigation
                        .store(navigation_id, Ordering::Release);
                    state.expected_generation.store(
                        state.generation.load(Ordering::Acquire),
                        Ordering::Release,
                    );
                    // 网页自己触发的整页导航也要进入 loading，才能被看门狗和
                    // 保留文档逻辑正确识别。
                    state.phase.store(PHASE_LOADING, Ordering::Release);
                    arm_navigation_timeout(&starting_app, navigation_id);
                    log(format!(
                        "event=navigation phase=started result=passed nav_id={navigation_id} allowed=true user_initiated={} redirected={} path={path} generation={}",
                        user_initiated.as_bool(),
                        redirected.as_bool(),
                        state.expected_generation.load(Ordering::Acquire)
                    ));
                    return Ok(());
                }

                // 站外地址：只有用户手势（点击）才交给系统浏览器；脚本重定向、
                // 自动跳转一律拦下，避免 App 在用户没点的情况下打开浏览器。
                let user_gesture = user_initiated_read && user_initiated.as_bool();
                if user_gesture {
                    if let Ok(url) = parsed.as_ref() {
                        open_external(&starting_app, url);
                    }
                }
                log(format!(
                    "event=navigation phase=started result=blocked reason={} nav_id={navigation_id} user_initiated={} redirected={} path={path}",
                    if user_gesture {
                        "user_initiated_external"
                    } else {
                        "non_user_initiated"
                    },
                    user_initiated.as_bool(),
                    redirected.as_bool()
                ));
                let _ = args.SetCancel(true);
                Ok(())
            }));

            let completed_app = app.clone();
            let completed_core = core.clone();
            let completed = NavigationCompletedEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    log("event=navigation phase=completed result=failed reason=missing_args"
                        .to_owned());
                    return Ok(());
                };
                let mut navigation_id = 0u64;
                if args.NavigationId(&mut navigation_id).is_err() {
                    log("event=navigation phase=completed result=failed reason=navigation_id"
                        .to_owned());
                    return Ok(());
                }
                let state = completed_app.state::<RecommendationsState>();
                let expected = state.expected_navigation.load(Ordering::Acquire);

                let mut success = BOOL(0);
                let success_read = args.IsSuccess(&mut success).is_ok();
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                let _ = args.WebErrorStatus(&mut status);
                log(format!(
                    "event=navigation phase=completed result=observed nav_id={navigation_id} expected={expected} success={} success_read={success_read} web_error_status={}",
                    success.as_bool(),
                    status.0
                ));

                if !consume_matching_navigation(&state, navigation_id) {
                    log(format!(
                        "event=navigation phase=completed result=ignored reason={} nav_id={navigation_id} expected={expected}",
                        if expected == 0 {
                            "no_pending_navigation"
                        } else {
                            "stale_navigation"
                        }
                    ));
                    return Ok(());
                }
                let generation = state.expected_generation.load(Ordering::Acquire);

                // 主文档 HTTP 状态单独判定：WebView2 对 4xx/5xx 仍报 IsSuccess。
                // 只在接受本次导航后消费，避免迟到的完成事件吃掉新导航的状态。
                let document_status = state.document_status.swap(0, Ordering::AcqRel);
                let http_ok = document_status == 0 || (200..400).contains(&document_status);

                if !(success_read && success.as_bool() && http_ok) {
                    state
                        .reset_history_after_navigation
                        .store(false, Ordering::Release);
                    state.phase.store(PHASE_FAILED, Ordering::Release);
                    state.visible.store(false, Ordering::Release);
                    if let Some(webview) = completed_app.get_webview(LABEL) {
                        let _ = webview.hide();
                    }
                    let reason = if http_ok {
                        "navigation_failed"
                    } else {
                        "http_error"
                    };
                    emit_failed(&completed_app, generation, reason);
                    log(format!(
                        "event=navigation phase=completed result=failed generation={generation} http_status={document_status} web_error_status={} reason={reason}",
                        status.0
                    ));
                    return Ok(());
                }

                // 主文档已加载，但 Mac 容器要求网页显式报告 ready；站点在
                // mount 结束时设置 window.sayallWebPageState。读不到时保持
                // loading，由前端 20s 超时兜底（与 Mac 行为一致）。
                //
                // App 自己发起的重试/换主题/换语言/首次加载不是用户浏览历史：
                // 成功后清空 WebView 历史，避免失败页或旧页面成为「返回上一页」
                // 目标（2026-10-24 用户报告：重试后顶部出现返回条）。
                let should_reset_history = state
                    .reset_history_after_navigation
                    .swap(false, Ordering::AcqRel);
                if should_reset_history {
                    state.history_reset_pending.store(true, Ordering::Release);
                    state.can_go_back.store(false, Ordering::Release);
                } else {
                    let mut can_go_back = BOOL(0);
                    if completed_core.CanGoBack(&mut can_go_back).is_ok() {
                        state
                            .can_go_back
                            .store(can_go_back.as_bool(), Ordering::Release);
                    }
                }
                state.phase.store(PHASE_LOADING, Ordering::Release);
                if should_reset_history {
                    let reset_app = completed_app.clone();
                    let reset_core = completed_core.clone();
                    let reset = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                        move |error, _json| {
                            let state = reset_app.state::<RecommendationsState>();
                            state.history_reset_pending.store(false, Ordering::Release);
                            let mut can_go_back = BOOL(0);
                            let can = reset_core.CanGoBack(&mut can_go_back).is_ok()
                                && can_go_back.as_bool();
                            state.can_go_back.store(can, Ordering::Release);
                            log(format!(
                                "event=history_reset phase=completed result={} can_go_back={can} generation={generation}",
                                if error.is_err() { "failed" } else { "passed" }
                            ));
                            if state.phase.load(Ordering::Acquire) == PHASE_READY {
                                emit_ready(&reset_app, state.generation.load(Ordering::Acquire));
                            }
                            Ok(())
                        },
                    ));
                    let method: Vec<u16> =
                        "Page.resetNavigationHistory\0".encode_utf16().collect();
                    let params: Vec<u16> = "{}\0".encode_utf16().collect();
                    if let Err(error) = completed_core.CallDevToolsProtocolMethod(
                        PCWSTR::from_raw(method.as_ptr()),
                        PCWSTR::from_raw(params.as_ptr()),
                        &reset,
                    ) {
                        state.history_reset_pending.store(false, Ordering::Release);
                        log(format!(
                            "event=history_reset phase=completed result=failed reason=cdp_call_error error={error} generation={generation}"
                        ));
                    }
                }
                let probe_app = completed_app.clone();
                let probe_core = completed_core.clone();
                let probe = ExecuteScriptCompletedHandler::create(Box::new(move |error, json| {
                    let state = probe_app.state::<RecommendationsState>();
                    if state.generation.load(Ordering::Acquire) != generation {
                        log(format!(
                            "event=ready_signal phase=completed result=ignored reason=stale_generation generation={generation}"
                        ));
                        return Ok(());
                    }
                    // 若已有更新的导航在等待，这次探测结果属于上一页，不能覆盖
                    // 新导航的 loading 状态（否则新导航的看门狗会失效）。
                    if state.expected_navigation.load(Ordering::Acquire) != 0 {
                        log(format!(
                            "event=ready_signal phase=completed result=ignored reason=newer_navigation generation={generation}"
                        ));
                        return Ok(());
                    }
                    if error.is_err() {
                        log(format!(
                            "event=ready_signal phase=completed result=failed reason=script_error generation={generation}"
                        ));
                        return Ok(());
                    }
                    let parsed: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                    let signal = parsed
                        .get("state")
                        .and_then(|value| value.as_str())
                        .unwrap_or("missing");
                    match signal {
                        "ready" => {
                            state.phase.store(PHASE_READY, Ordering::Release);
                            state.visible.store(true, Ordering::Release);
                            if let Some(webview) = probe_app.get_webview(LABEL) {
                                if let Some((position, size)) = layout(&probe_app) {
                                    let _ = webview.set_position(position);
                                    let _ = webview.set_size(size);
                                }
                                let _ = webview.show();
                            }
                            emit_ready(&probe_app, generation);
                            log(format!(
                                "event=ready_signal phase=completed result=passed reason=document_ready generation={generation} http_status={document_status}"
                            ));
                        }
                        "failed" => {
                            state.phase.store(PHASE_FAILED, Ordering::Release);
                            state.visible.store(false, Ordering::Release);
                            if let Some(webview) = probe_app.get_webview(LABEL) {
                                let _ = webview.hide();
                            }
                            emit_failed(&probe_app, generation, "page_script");
                            log(format!(
                                "event=ready_signal phase=completed result=failed reason=page_script generation={generation}"
                            ));
                        }
                        _ => {
                            log(format!(
                                "event=ready_signal phase=completed result=deferred reason=missing generation={generation}"
                            ));
                        }
                    }
                    Ok(())
                }));
                let script = "(function(){try{return {state: window.sayallWebPageState || null};}catch(e){return {state: null};}})()";
                let script_utf16: Vec<u16> =
                    script.encode_utf16().chain(std::iter::once(0)).collect();
                if let Err(error) =
                    probe_core.ExecuteScript(PCWSTR::from_raw(script_utf16.as_ptr()), &probe)
                {
                    log(format!(
                        "event=ready_signal phase=completed result=failed reason=execute_script_error generation={generation} error={error}"
                    ));
                }
                Ok(())
            }));

            let mut starting_token = 0;
            let starting_result = core.add_NavigationStarting(&starting, &mut starting_token);

            // 子 frame 一律不加载：与 Mac 容器 `targetFrame.isMainFrame == false`
            // 时取消的规则一致，避免第三方 iframe 绕过地址白名单。
            let subframe = NavigationStartingEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let mut uri = PWSTR::null();
                let same_host = args.Uri(&mut uri).is_ok()
                    && Url::parse(&CoTaskMemPWSTR::from(uri).to_string())
                        .map(|url| url.host_str() == Some("sayall.app"))
                        .unwrap_or(false);
                let _ = args.SetCancel(true);
                log(format!(
                    "event=subframe_navigation phase=completed result=blocked same_host={same_host}"
                ));
                Ok(())
            }));
            let mut subframe_token = 0;
            let subframe_result = core.add_FrameNavigationStarting(&subframe, &mut subframe_token);

            // 主文档响应状态：WebView2 对 4xx/5xx 仍报 `IsSuccess`，必须单独读
            // HTTP 状态。只认推荐页地址的主文档请求，避免把图片/接口的 4xx
            // 当成整页失败（产品规范：图片单项失败不算整页失败）。
            let response_app = app.clone();
            let response = webview2_com::WebResourceResponseReceivedEventHandler::create(
                Box::new(move |_sender, args| {
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

            let failed_app = app.clone();
            let process_failed = ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let mut kind =
                    webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND(
                        0,
                    );
                if args.ProcessFailedKind(&mut kind).is_err() {
                    return Ok(());
                }
                let fatal = kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                    || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
                    || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;
                let state = failed_app.state::<RecommendationsState>();
                let generation = state.generation.load(Ordering::Acquire);
                log(format!(
                    "event=process_failed phase=completed result=observed kind={} fatal={fatal} generation={generation}",
                    kind.0
                ));
                if fatal {
                    state
                        .reset_history_after_navigation
                        .store(false, Ordering::Release);
                    state.phase.store(PHASE_FAILED, Ordering::Release);
                    state.visible.store(false, Ordering::Release);
                    if let Some(webview) = failed_app.get_webview(LABEL) {
                        let _ = webview.hide();
                    }
                    emit_failed(&failed_app, generation, "process_failed");
                }
                Ok(())
            }));
            let mut process_token = 0;
            let process_result = core.add_ProcessFailed(&process_failed, &mut process_token);

            // 历史变化（含站内 pushState 的列表/详情切换）决定「返回上一页」条。
            let history_app = app.clone();
            let history_core = core.clone();
            let history = HistoryChangedEventHandler::create(Box::new(move |_sender, _args| {
                let mut can_go_back = BOOL(0);
                let actual =
                    history_core.CanGoBack(&mut can_go_back).is_ok() && can_go_back.as_bool();
                let state = history_app.state::<RecommendationsState>();
                let reset_pending = state.history_reset_pending.load(Ordering::Acquire);
                let can = actual && !reset_pending;
                let phase = state.phase.load(Ordering::Acquire);
                let changed = state.can_go_back.swap(can, Ordering::AcqRel) != can;
                if changed && phase == PHASE_READY {
                    if let Some(webview) = history_app.get_webview(LABEL) {
                        if let Some((position, size)) = layout(&history_app) {
                            let _ = webview.set_position(position);
                            let _ = webview.set_size(size);
                        }
                    }
                }
                if phase == PHASE_READY {
                    emit_ready(&history_app, state.generation.load(Ordering::Acquire));
                }
                log(format!(
                    "event=history_changed phase=completed result=passed can_go_back={can} changed={changed} reset_pending={reset_pending} generation={}",
                    state.generation.load(Ordering::Acquire)
                ));
                Ok(())
            }));
            let mut history_token = 0;
            let history_result = core.add_HistoryChanged(&history, &mut history_token);

            let mut completed_token = 0;
            let completed_result =
                core.add_NavigationCompleted(&completed, &mut completed_token);
            let result = starting_result
                .and(completed_result)
                .and(response_result)
                .and(process_result)
                .and(history_result)
                .and(subframe_result);
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
    fn embedded_page_url_carries_app_mode_theme_and_language() {
        let light = page_url("light", "zh-Hans");
        assert_eq!(
            light.as_str(),
            "https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=light"
        );
        let dark = page_url("dark", "en");
        assert_eq!(
            dark.as_str(),
            "https://sayall.app/en/recommendations/?embed=app&appPlatform=windows&theme=dark"
        );
        // 未知主题按浅色处理，未知语言按中文处理，不把任意字符串写进 URL。
        assert!(page_url("system", "fr")
            .as_str()
            .ends_with("recommendations/?embed=app&appPlatform=windows&theme=light"));
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
        let target = browser_destination(Some(&current), "zh-Hans");
        assert_eq!(
            target.as_str(),
            "https://sayall.app/recommendations/?item=vokie"
        );

        let english = Url::parse(
            "https://sayall.app/en/recommendations/?embed=app&theme=light&item=xiaomi-2",
        )
        .unwrap();
        assert_eq!(
            browser_destination(Some(&english), "en").as_str(),
            "https://sayall.app/en/recommendations/?item=xiaomi-2"
        );
    }

    #[test]
    fn browser_destination_falls_back_to_list_in_the_selected_language() {
        let other = Url::parse("https://sayall.app/market/").unwrap();
        assert_eq!(
            browser_destination(Some(&other), "zh-Hans").as_str(),
            "https://sayall.app/recommendations/"
        );
        assert_eq!(
            browser_destination(None, "zh-Hans").as_str(),
            "https://sayall.app/recommendations/"
        );
        assert_eq!(
            browser_destination(None, "en").as_str(),
            "https://sayall.app/en/recommendations/"
        );
        let blank = Url::parse("about:blank").unwrap();
        assert_eq!(
            browser_destination(Some(&blank), "en").as_str(),
            "https://sayall.app/en/recommendations/"
        );
    }

    /// 迟到/不匹配的完成事件不能清掉当前等待中的导航。
    ///
    /// 旧实现先 `swap(0)` 再比对：旧导航的完成事件会把新导航的等待 ID 吞掉，
    /// 新导航真正完成时被当成迟到事件忽略，推荐页永远停在加载中。
    #[test]
    fn stale_completion_does_not_clear_pending_navigation() {
        let state = RecommendationsState::default();
        state.expected_navigation.store(7, Ordering::Release);

        assert!(
            !consume_matching_navigation(&state, 3),
            "旧导航的完成事件必须被忽略"
        );
        assert_eq!(
            state.expected_navigation.load(Ordering::Acquire),
            7,
            "忽略迟到事件时不能清掉新导航的等待 ID"
        );

        assert!(consume_matching_navigation(&state, 7));
        assert_eq!(state.expected_navigation.load(Ordering::Acquire), 0);

        assert!(
            !consume_matching_navigation(&state, 7),
            "同一导航的第二次完成事件不能再被接受"
        );
    }

    #[test]
    fn retained_document_only_when_ready_and_appearance_matches() {
        assert_eq!(
            show_intent(PHASE_READY, true, false),
            ShowIntent::RetainedReady
        );
        assert_eq!(
            show_intent(PHASE_FAILED, true, false),
            ShowIntent::RetainedFailed
        );
        assert_eq!(show_intent(PHASE_READY, false, false), ShowIntent::Navigate);
        assert_eq!(show_intent(PHASE_READY, true, true), ShowIntent::Navigate);
        assert_eq!(
            show_intent(PHASE_LOADING, true, false),
            ShowIntent::Navigate
        );
        assert_eq!(show_intent(PHASE_IDLE, true, false), ShowIntent::Navigate);
    }

    #[test]
    fn retry_preserves_current_detail_and_applies_theme_and_language() {
        let current = Url::parse(
            "https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=light&item=vokie&from=sayall.app",
        )
        .unwrap();
        assert_eq!(
            embedded_destination(Some(&current), "dark", "zh-Hans").as_str(),
            "https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=dark&item=vokie&from=sayall.app"
        );

        let english = Url::parse("https://sayall.app/en/recommendations/?item=xiaomi-2").unwrap();
        assert_eq!(
            embedded_destination(Some(&english), "light", "en").as_str(),
            "https://sayall.app/en/recommendations/?embed=app&appPlatform=windows&theme=light&item=xiaomi-2"
        );
        // 语言切换保留同一个详情，只换站内语言前缀。
        assert_eq!(
            embedded_destination(Some(&english), "light", "zh-Hans").as_str(),
            "https://sayall.app/recommendations/?embed=app&appPlatform=windows&theme=light&item=xiaomi-2"
        );
    }

    #[test]
    fn retry_falls_back_to_the_list_for_unembedded_pages() {
        for current in [
            Url::parse("about:blank").unwrap(),
            Url::parse("https://example.com/recommendations/").unwrap(),
        ] {
            assert_eq!(
                embedded_destination(Some(&current), "dark", "zh-Hans").as_str(),
                page_url("dark", "zh-Hans").as_str()
            );
        }
        assert_eq!(
            embedded_destination(None, "light", "en").as_str(),
            page_url("light", "en").as_str()
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
