import Foundation
import TelemetryDeck

enum AppAnalytics {
    private static var isConfigured = false

    static func configure() {
        guard !isConfigured else { return }
        let appID = ProcessInfo.processInfo.environment["VREMOTER_TELEMETRY_APP_ID"]
            ?? Bundle.main.object(forInfoDictionaryKey: "VRTelemetryDeckAppID") as? String
            ?? ""
        guard !appID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            print("[ANALYTICS] TelemetryDeck App ID is not configured")
            return
        }

        let namespace = ProcessInfo.processInfo.environment["VREMOTER_TELEMETRY_NAMESPACE"]
            ?? Bundle.main.object(forInfoDictionaryKey: "VRTelemetryDeckNamespace") as? String
        let config = TelemetryDeck.Config(appID: appID, namespace: namespace)
        if ProcessInfo.processInfo.environment["VREMOTER_TELEMETRY_DEBUG"] == "1" {
            config.logHandler = LogHandler(logLevel: .debug) { level, message in
                let line = "[TelemetryDeck: \(level)] \(message)\n"
                FileHandle.standardError.write(Data(line.utf8))
            }
        }
        config.defaultSignalPrefix = "vRemoter."
        config.defaultParameters = {
            [
                "language": AppLanguage.selected.rawValue,
                "releaseChannel": "stable"
            ]
        }
        TelemetryDeck.initialize(config: config)
        isConfigured = true
        signal("App.launched")
    }

    static func signal(_ name: String, parameters: [String: String] = [:]) {
        guard isConfigured else { return }
        TelemetryDeck.signal(name, parameters: parameters)
    }
}
