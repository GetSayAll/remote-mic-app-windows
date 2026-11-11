import AppKit
import Foundation

struct CommerceStore: Codable, Identifiable, Equatable {
    let id: String
    let enabled: Bool
    let nameZh: String
    let nameEn: String
    let url: String
    let qrImageURL: String

    var title: String { L10n.text(nameZh, nameEn) }
    var purchaseURL: URL? { validatedWebURL(url) }
    var remoteQRCodeURL: URL? { validatedWebURL(qrImageURL) }

    private func validatedWebURL(_ value: String) -> URL? {
        guard let url = URL(string: value),
              let scheme = url.scheme?.lowercased(),
              ["http", "https"].contains(scheme),
              url.host != nil else { return nil }
        return url
    }
}

struct CommerceConfig: Codable, Equatable {
    let schemaVersion: Int
    let updatedAt: String
    let heroImageURL: String
    let stores: [CommerceStore]

    var availableStores: [CommerceStore] {
        stores.filter { $0.enabled && $0.purchaseURL != nil }
    }
}

enum CommerceAsset {
    static func image(named name: String) -> NSImage? {
        let candidates: [URL?] = [
            Bundle.main.resourceURL?
                .appendingPathComponent("Commerce", isDirectory: true)
                .appendingPathComponent(name),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                .appendingPathComponent("Resources/Commerce/\(name)")
        ]
        for candidate in candidates.compactMap({ $0 }) {
            if let image = NSImage(contentsOf: candidate) { return image }
        }
        return nil
    }
}

@MainActor
final class CommerceConfigModel: ObservableObject {
    static let shared = CommerceConfigModel()

    @Published private(set) var config: CommerceConfig
    @Published private(set) var isRefreshing = false

    private let decoder = JSONDecoder()
    private let remoteURL: URL?
    private let cacheURL: URL

    private init() {
        let fallback = Self.loadBundledConfig() ?? CommerceConfig(
            schemaVersion: 1,
            updatedAt: "fallback",
            heroImageURL: "",
            stores: []
        )
        cacheURL = AppStorage.applicationSupportDirectory
            .appendingPathComponent("commerce.json")
        let configuredURL = ProcessInfo.processInfo.environment["VREMOTER_COMMERCE_CONFIG_URL"]
            ?? Bundle.main.object(forInfoDictionaryKey: "VRCommerceConfigURL") as? String
        remoteURL = configuredURL.flatMap(URL.init(string:))

        let cachedDecoder = JSONDecoder()
        let cached = (try? Data(contentsOf: cacheURL))
            .flatMap { try? cachedDecoder.decode(CommerceConfig.self, from: $0) }
        config = Self.preferredConfig(bundled: fallback, cached: cached)
    }

    func refresh() {
        guard !isRefreshing, let remoteURL else { return }
        isRefreshing = true
        Task {
            defer { isRefreshing = false }
            do {
                let data: Data
                if remoteURL.isFileURL {
                    data = try Data(contentsOf: remoteURL)
                } else {
                    var request = URLRequest(url: remoteURL)
                    request.cachePolicy = .reloadIgnoringLocalCacheData
                    request.timeoutInterval = 8
                    let (downloaded, response) = try await URLSession.shared.data(for: request)
                    guard let http = response as? HTTPURLResponse,
                          (200..<300).contains(http.statusCode) else {
                        throw URLError(.badServerResponse)
                    }
                    data = downloaded
                }
                let updated = try decoder.decode(CommerceConfig.self, from: data)
                guard updated.schemaVersion == 1 else { return }
                config = updated
                try? data.write(to: cacheURL, options: .atomic)
            } catch {
                print("[COMMERCE] refresh failed: \(error.localizedDescription)")
            }
        }
    }

    private static func loadBundledConfig() -> CommerceConfig? {
        let candidates: [URL?] = [
            Bundle.main.resourceURL?
                .appendingPathComponent("Commerce", isDirectory: true)
                .appendingPathComponent("commerce.json"),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                .appendingPathComponent("Resources/Commerce/commerce.json")
        ]
        for url in candidates.compactMap({ $0 }) {
            guard let data = try? Data(contentsOf: url) else { continue }
            if let config = try? JSONDecoder().decode(CommerceConfig.self, from: data) {
                return config
            }
        }
        return nil
    }

    private static func preferredConfig(
        bundled: CommerceConfig,
        cached: CommerceConfig?
    ) -> CommerceConfig {
        guard let cached, cached.schemaVersion == 1 else { return bundled }
        let formatter = ISO8601DateFormatter()
        guard let bundledDate = formatter.date(from: bundled.updatedAt),
              let cachedDate = formatter.date(from: cached.updatedAt) else {
            return bundled
        }
        return cachedDate > bundledDate ? cached : bundled
    }
}
