import Foundation

enum AppLanguage: String, CaseIterable {
    case system
    case simplifiedChinese = "zh-Hans"
    case english = "en"

    private static let defaultsKey = "vRemoter.appLanguage"

    static var selected: AppLanguage {
        get {
            guard let raw = UserDefaults.standard.string(forKey: defaultsKey),
                  let language = AppLanguage(rawValue: raw) else {
                return .system
            }
            return language
        }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: defaultsKey) }
    }

    var usesEnglish: Bool {
        switch self {
        case .english:
            return true
        case .simplifiedChinese:
            return false
        case .system:
            return !(Locale.preferredLanguages.first ?? "zh-Hans").hasPrefix("zh")
        }
    }
}

enum L10n {
    static func text(_ simplifiedChinese: String, _ english: String) -> String {
        AppLanguage.selected.usesEnglish ? english : simplifiedChinese
    }
}
