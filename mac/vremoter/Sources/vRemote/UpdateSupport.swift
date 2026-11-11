import AppKit
import CryptoKit
import Foundation
import SwiftUI

private struct GitHubReleaseAsset: Decodable, Identifiable {
    let id: Int
    let name: String
    let browserDownloadURL: URL
    let digest: String?

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case browserDownloadURL = "browser_download_url"
        case digest
    }
}

private struct GitHubRelease: Decodable, Identifiable {
    let id: Int
    let tagName: String
    let name: String?
    let body: String?
    let htmlURL: URL
    let draft: Bool
    let prerelease: Bool
    let driverUpdateRequired: Bool?
    let assets: [GitHubReleaseAsset]

    enum CodingKeys: String, CodingKey {
        case id
        case tagName = "tag_name"
        case name
        case body
        case htmlURL = "html_url"
        case draft
        case prerelease
        case driverUpdateRequired = "driver_update_required"
        case assets
    }

    var displayVersion: String {
        tagName.hasPrefix("v") ? String(tagName.dropFirst()) : tagName
    }

    var installerAsset: GitHubReleaseAsset? {
        let package = assets.first { $0.name.lowercased().hasSuffix(".pkg") }
        let diskImage = assets.first { $0.name.lowercased().hasSuffix(".dmg") }
        return driverUpdateRequired == true ? (package ?? diskImage) : (diskImage ?? package)
    }
}

private struct AppVersion: Comparable {
    let numbers: [Int]
    let prerelease: [String]?

    init(_ rawValue: String) {
        let cleaned = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "vV"))
        let parts = cleaned.split(separator: "-", maxSplits: 1).map(String.init)
        var parsed = parts[0].split(separator: ".").map { Int($0) ?? 0 }
        while parsed.count < 3 { parsed.append(0) }
        numbers = parsed
        prerelease = parts.count > 1
            ? parts[1].split(separator: ".").map(String.init)
            : nil
    }

    static func < (lhs: AppVersion, rhs: AppVersion) -> Bool {
        let count = max(lhs.numbers.count, rhs.numbers.count)
        for index in 0..<count {
            let left = index < lhs.numbers.count ? lhs.numbers[index] : 0
            let right = index < rhs.numbers.count ? rhs.numbers[index] : 0
            if left != right { return left < right }
        }
        switch (lhs.prerelease, rhs.prerelease) {
        case (nil, nil): return false
        case (nil, _): return false
        case (_, nil): return true
        case (.some(let left), .some(let right)):
            let count = max(left.count, right.count)
            for index in 0..<count {
                guard index < left.count else { return true }
                guard index < right.count else { return false }
                let result = left[index].compare(
                    right[index],
                    options: [.numeric, .caseInsensitive]
                )
                if result != .orderedSame { return result == .orderedAscending }
            }
            return false
        }
    }
}

private enum UpdateCheckResult {
    case upToDate
    case available(GitHubRelease)
}

private enum UpdateServiceError: Error {
    case sourceUnavailable
}

private enum UpdateDialog: Identifiable {
    case upToDate
    case available(GitHubRelease)
    case failure(String)

    var id: String {
        switch self {
        case .upToDate: "up-to-date"
        case .available(let release): "available-\(release.id)"
        case .failure: "failure"
        }
    }
}

private actor GitHubUpdateService {
    private let releasesURL: URL
    private let currentVersion: String

    init(releasesURL configuredURL: URL, currentVersion: String) {
        self.currentVersion = currentVersion
        if let override = ProcessInfo.processInfo.environment["VREMOTER_RELEASES_URL"],
           let url = URL(string: override) {
            releasesURL = url
        } else {
            releasesURL = configuredURL
        }
    }

    func checkForUpdate() async throws -> UpdateCheckResult {
        let data: Data
        if releasesURL.isFileURL {
            data = try Data(contentsOf: releasesURL)
        } else {
            var request = URLRequest(url: releasesURL)
            request.timeoutInterval = 15
            request.setValue("application/json", forHTTPHeaderField: "Accept")
            request.setValue("vRemoter/\(currentVersion)", forHTTPHeaderField: "User-Agent")

            let (responseData, response) = try await URLSession.shared.data(for: request)
            guard let http = response as? HTTPURLResponse else {
                throw URLError(.badServerResponse)
            }
            if http.statusCode == 404 { throw UpdateServiceError.sourceUnavailable }
            guard (200..<300).contains(http.statusCode) else {
                throw URLError(.badServerResponse)
            }
            data = responseData
        }

        let releases = try JSONDecoder().decode([GitHubRelease].self, from: data)
        let current = AppVersion(currentVersion)
        let acceptsPrereleases = currentVersion.contains("-")
        let newest = releases
            .filter { !$0.draft && (acceptsPrereleases || !$0.prerelease) }
            .max { AppVersion($0.tagName) < AppVersion($1.tagName) }

        guard let newest, AppVersion(newest.tagName) > current else {
            return .upToDate
        }
        return .available(newest)
    }

    func downloadInstaller(for release: GitHubRelease) async throws -> URL? {
        guard let asset = release.installerAsset else { return nil }
        var request = URLRequest(url: asset.browserDownloadURL)
        request.timeoutInterval = 120
        request.setValue("application/octet-stream", forHTTPHeaderField: "Accept")
        request.setValue("vRemoter/\(currentVersion)", forHTTPHeaderField: "User-Agent")

        let (temporaryURL, response) = try await URLSession.shared.download(for: request)
        guard let http = response as? HTTPURLResponse,
              (200..<300).contains(http.statusCode) else {
            throw URLError(.badServerResponse)
        }

        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("vRemoter-Updates", isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory,
            withIntermediateDirectories: true
        )
        let destination = directory.appendingPathComponent(asset.name)
        if FileManager.default.fileExists(atPath: destination.path) {
            try FileManager.default.removeItem(at: destination)
        }
        try FileManager.default.moveItem(at: temporaryURL, to: destination)

        if let digest = asset.digest,
           digest.lowercased().hasPrefix("sha256:") {
            let expected = String(digest.dropFirst("sha256:".count)).lowercased()
            let data = try Data(contentsOf: destination)
            let actual = SHA256.hash(data: data)
                .map { String(format: "%02x", $0) }
                .joined()
            guard actual == expected else {
                try? FileManager.default.removeItem(at: destination)
                throw CocoaError(.fileReadCorruptFile)
            }
        }
        return destination
    }
}

@MainActor
private final class UpdateWindowModel: ObservableObject {
    @Published var isChecking = false
    @Published var isDownloading = false
    @Published var dialog: UpdateDialog?

    let currentVersion: String
    private let service: GitHubUpdateService
    private static let automaticUpdatesKey = "vRemoter.automaticUpdates"

    var automaticUpdates: Bool {
        get {
            let defaults = UserDefaults.standard
            guard defaults.object(forKey: Self.automaticUpdatesKey) != nil else { return true }
            return defaults.bool(forKey: Self.automaticUpdatesKey)
        }
        set {
            objectWillChange.send()
            UserDefaults.standard.set(newValue, forKey: Self.automaticUpdatesKey)
        }
    }

    init(releasesURL: URL, currentVersion: String) {
        self.currentVersion = currentVersion
        service = GitHubUpdateService(
            releasesURL: releasesURL,
            currentVersion: currentVersion
        )
    }

    func checkManually() {
        guard !isChecking else { return }
        AppAnalytics.signal("Update.manualCheck")
        isChecking = true
        Task {
            do {
                let result = try await service.checkForUpdate()
                isChecking = false
                present(result)
            } catch {
                isChecking = false
                if case UpdateServiceError.sourceUnavailable = error {
                    dialog = .failure(L10n.text(
                        "更新服务尚未上线，暂时无法检查版本。",
                        "The update service is not available yet."
                    ))
                } else {
                    dialog = .failure(L10n.text(
                        "无法连接更新服务器，请检查网络后重试。",
                        "Could not reach the update server. Check your connection and try again."
                    ))
                }
            }
        }
    }

    func automaticResult() async -> UpdateCheckResult? {
        guard automaticUpdates else { return nil }
        do {
            return try await service.checkForUpdate()
        } catch {
            return nil
        }
    }

    func present(_ result: UpdateCheckResult) {
        switch result {
        case .upToDate: dialog = .upToDate
        case .available(let release): dialog = .available(release)
        }
    }

    func install(_ release: GitHubRelease) {
        guard !isDownloading else { return }
        AppAnalytics.signal(
            "Update.installSelected",
            parameters: ["version": release.displayVersion]
        )
        isDownloading = true
        Task {
            do {
                if let installer = try await service.downloadInstaller(for: release) {
                    isDownloading = false
                    dialog = nil
                    NSWorkspace.shared.open(installer)
                } else {
                    isDownloading = false
                    dialog = nil
                    NSWorkspace.shared.open(release.htmlURL)
                }
            } catch {
                isDownloading = false
                dialog = .failure(L10n.text(
                    "更新包下载或校验失败，请稍后重试。",
                    "The update could not be downloaded or verified. Try again later."
                ))
            }
        }
    }

    func showDemoUpdate() {
        let json = """
        {
          "id": 999999,
          "tag_name": "v1.1.1",
          "name": "vRemoter 1.1.1",
          "body": "- 修复 Chromecast 短按偶尔未持续开启遥控器麦克风的问题\\n- 修复电脑 Option 关闭后旧状态可能重新开启遥控器音频的问题\\n- 改进双遥控器会话隔离与开麦确认\\n- 修复 X6 第一次短按可能被误判的问题",
          "html_url": "https://updates.vincentstudio.org/vremoter/",
          "draft": false,
          "prerelease": false,
          "driver_update_required": false,
          "assets": []
        }
        """
        if let data = json.data(using: .utf8),
           let release = try? JSONDecoder().decode(GitHubRelease.self, from: data) {
            dialog = .available(release)
        }
    }

    func showUpToDateDemo() {
        dialog = .upToDate
    }
}

final class UpdateWindowController: NSWindowController, NSWindowDelegate {
    private let model: UpdateWindowModel

    init() {
        let info = Bundle.main.infoDictionary ?? [:]
        let releasesURL = (info["VRReleasesURL"] as? String)
            .flatMap(URL.init(string:))
            ?? URL(string: "https://updates.vincentstudio.org/vremoter/releases.json")!
        let version = info["CFBundleShortVersionString"] as? String ?? "development"
        model = UpdateWindowModel(releasesURL: releasesURL, currentVersion: version)

        let size = NSSize(width: 500, height: 440)
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = L10n.text("版本与更新", "Version & Updates")
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.isMovableByWindowBackground = true
        window.isReleasedWhenClosed = false
        window.backgroundColor = NSColor(
            red: 0.055,
            green: 0.059,
            blue: 0.067,
            alpha: 1
        )
        window.appearance = NSAppearance(named: .darkAqua)
        window.minSize = size
        window.maxSize = size
        window.center()
        window.contentView = NSHostingView(
            rootView: VersionUpdateView(model: model)
                .preferredColorScheme(.dark)
        )
        super.init(window: window)
        window.delegate = self
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func show() {
        guard let window else { return }
        print("[UPDATE] showing version window")
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func checkAutomatically() {
        Task { @MainActor [weak self] in
            guard let self,
                  let result = await self.model.automaticResult(),
                  case .available = result else { return }
            self.show()
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) {
                self.model.present(result)
            }
        }
    }

    func showDemoUpdate() {
        show()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [model] in
            model.showDemoUpdate()
        }
    }

    func showUpToDateDemo() {
        show()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [model] in
            model.showUpToDateDemo()
        }
    }
}

private enum UpdateTheme {
    static let panel = Color(red: 0.055, green: 0.059, blue: 0.067)
    static let surface = Color(red: 0.085, green: 0.092, blue: 0.105)
    static let line = Color(red: 0.20, green: 0.22, blue: 0.25)
    static let text = Color(red: 0.94, green: 0.95, blue: 0.96)
    static let secondary = Color(red: 0.57, green: 0.60, blue: 0.66)
    static let green = Color(red: 0.30, green: 0.82, blue: 0.53)
}

private struct VersionUpdateView: View {
    @ObservedObject var model: UpdateWindowModel

    var body: some View {
        VStack(spacing: 0) {
            Spacer().frame(height: 42)
            Image(nsImage: LogoAsset.image)
                .resizable()
                .interpolation(.high)
                .frame(width: 116, height: 116)
            Text("vRemoter")
                .font(.system(size: 28, weight: .semibold))
                .foregroundStyle(UpdateTheme.text)
                .padding(.top, 18)
            Text(L10n.text("版本 \(model.currentVersion)", "Version \(model.currentVersion)"))
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(UpdateTheme.secondary)
                .padding(.top, 6)
            Spacer()
            Button {
                model.checkManually()
            } label: {
                HStack(spacing: 8) {
                    if model.isChecking {
                        ProgressView().controlSize(.small)
                    }
                    Text(model.isChecking
                        ? L10n.text("正在检查…", "Checking…")
                        : L10n.text("检查更新", "Check for Updates"))
                }
                .frame(width: 180, height: 40)
            }
            .buttonStyle(UpdateButtonStyle(primary: true))
            .disabled(model.isChecking)

            Toggle(
                L10n.text("自动更新", "Automatic updates"),
                isOn: Binding(
                    get: { model.automaticUpdates },
                    set: { model.automaticUpdates = $0 }
                )
            )
            .toggleStyle(.checkbox)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(UpdateTheme.text)
            .padding(.top, 18)
            Spacer().frame(height: 34)
        }
        .frame(width: 500, height: 440)
        .background(UpdateTheme.panel)
        .sheet(item: $model.dialog) { dialog in
            switch dialog {
            case .upToDate:
                UpToDateView(onClose: { model.dialog = nil })
            case .available(let release):
                UpdateAvailableView(
                    release: release,
                    isDownloading: model.isDownloading,
                    onLater: { model.dialog = nil },
                    onUpdate: { model.install(release) }
                )
            case .failure(let message):
                UpdateFailureView(message: message, onClose: { model.dialog = nil })
            }
        }
    }
}

private struct UpToDateView: View {
    let onClose: () -> Void

    var body: some View {
        VStack(spacing: 22) {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 58))
                .foregroundStyle(UpdateTheme.green)
            Text(L10n.text("当前已是最新版本", "You’re up to date"))
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(UpdateTheme.text)
            Button(L10n.text("关闭", "Close"), action: onClose)
                .buttonStyle(UpdateButtonStyle(primary: true))
        }
        .frame(width: 430, height: 280)
        .background(UpdateTheme.panel)
    }
}

private struct UpdateAvailableView: View {
    let release: GitHubRelease
    let isDownloading: Bool
    let onLater: () -> Void
    let onUpdate: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 14) {
                Image(nsImage: LogoAsset.image)
                    .resizable()
                    .frame(width: 52, height: 52)
                VStack(alignment: .leading, spacing: 3) {
                    Text(L10n.text("发现新版本", "A new version is available"))
                        .font(.system(size: 21, weight: .semibold))
                    Text(L10n.text(
                        "最新版本 \(release.displayVersion)",
                        "Latest version \(release.displayVersion)"
                    ))
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(UpdateTheme.green)
                }
            }

            Text(L10n.text("更新日志", "What’s new"))
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(UpdateTheme.secondary)
            ScrollView {
                Text(release.body?.isEmpty == false
                    ? release.body!
                    : L10n.text("本版本暂无更新说明。", "No release notes were provided."))
                    .font(.system(size: 13))
                    .foregroundStyle(UpdateTheme.text)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
                    .padding(14)
            }
            .frame(height: 190)
            .background(UpdateTheme.surface)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(UpdateTheme.line))

            HStack {
                Spacer()
                Button(L10n.text("下次再说", "Later"), action: onLater)
                    .buttonStyle(UpdateButtonStyle(primary: false))
                    .disabled(isDownloading)
                Button(action: onUpdate) {
                    HStack(spacing: 7) {
                        if isDownloading { ProgressView().controlSize(.small) }
                        Text(isDownloading
                            ? L10n.text("正在下载…", "Downloading…")
                            : L10n.text("立即更新", "Update Now"))
                    }
                }
                .buttonStyle(UpdateButtonStyle(primary: true))
                .disabled(isDownloading)
            }
        }
        .padding(26)
        .frame(width: 560, height: 430)
        .background(UpdateTheme.panel)
        .foregroundStyle(UpdateTheme.text)
    }
}

private struct UpdateFailureView: View {
    let message: String
    let onClose: () -> Void

    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(size: 48))
                .foregroundStyle(.orange)
            Text(message)
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(UpdateTheme.text)
                .multilineTextAlignment(.center)
            Button(L10n.text("关闭", "Close"), action: onClose)
                .buttonStyle(UpdateButtonStyle(primary: true))
        }
        .padding(28)
        .frame(width: 430, height: 280)
        .background(UpdateTheme.panel)
    }
}

private struct UpdateButtonStyle: ButtonStyle {
    let primary: Bool

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(primary ? Color.black : UpdateTheme.text)
            .padding(.horizontal, 20)
            .frame(minWidth: 110, minHeight: 38)
            .background(
                primary
                    ? UpdateTheme.green.opacity(configuration.isPressed ? 0.72 : 1)
                    : UpdateTheme.surface.opacity(configuration.isPressed ? 0.72 : 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(primary ? UpdateTheme.green : UpdateTheme.line)
            )
    }
}
