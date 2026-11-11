import AppKit
import Foundation

enum DonationAsset {
    static func image(named name: String) -> NSImage? {
        let candidates: [URL?] = [
            Bundle.main.resourceURL?
                .appendingPathComponent("BuyMeACoffee", isDirectory: true)
                .appendingPathComponent(name),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                .appendingPathComponent("Resources/buymeacoffee/\(name)")
        ]
        for candidate in candidates.compactMap({ $0 }) {
            if let image = NSImage(contentsOf: candidate) { return image }
        }
        return nil
    }
}

enum DonationPromptSchedule {
    private static let observedVersionKey = "vRemoter.donation.observedVersion"
    private static let eligibleDateKey = "vRemoter.donation.eligibleDate"
    private static let handledVersionKey = "vRemoter.donation.handledVersion"

    static var currentVersion: String {
        Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
            ?? "development"
    }

    static func shouldPresent(isHealthy: Bool, now: Date = Date()) -> Bool {
        guard isHealthy else { return false }
        let defaults = UserDefaults.standard
        let version = currentVersion

        if defaults.string(forKey: observedVersionKey) != version {
            defaults.set(version, forKey: observedVersionKey)
            defaults.set(now, forKey: eligibleDateKey)
            return false
        }

        guard defaults.string(forKey: handledVersionKey) != version else { return false }
        guard let eligibleDate = defaults.object(forKey: eligibleDateKey) as? Date else {
            defaults.set(now, forKey: eligibleDateKey)
            return false
        }

        let calendar = Calendar.current
        let eligibleDay = calendar.startOfDay(for: eligibleDate)
        let currentDay = calendar.startOfDay(for: now)
        guard currentDay > eligibleDay else { return false }

        return true
    }

    static func markHandled() {
        UserDefaults.standard.set(currentVersion, forKey: handledVersionKey)
    }
}
