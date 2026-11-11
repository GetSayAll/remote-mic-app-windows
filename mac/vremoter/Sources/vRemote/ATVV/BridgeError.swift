// Copyright (c) 2026 FanXeon@Poemcoder with Codex
// Minimal shim of mi-ao's BridgeError. We only need protocolFailure.
import Foundation

enum BridgeError: LocalizedError {
    case protocolFailure(String)

    var errorDescription: String? {
        switch self {
        case .protocolFailure(let message):
            return message
        }
    }
}
