import AppKit
import CoreAudio
import Foundation

/// The input method whose voice recognition vRemoter drives.
///
/// Doubao lets the user pick vRemoteDr 2ch in its own settings. WeType
/// (微信输入法) follows the system default input, so selecting it also makes
/// vRemoter route the system default input to vRemoteDr 2ch.
enum VoiceInputMethod: String, CaseIterable, Identifiable {
    case doubao
    case wetype

    var id: String { rawValue }

    var bundleIdentifier: String {
        switch self {
        case .doubao: "com.bytedance.inputmethod.doubaoime"
        case .wetype: "com.tencent.inputmethod.wetype"
        }
    }

    /// Short name used inside status sentences, e.g. "豆包录音中".
    var shortName: String {
        switch self {
        case .doubao: L10n.text("豆包", "Doubao")
        case .wetype: L10n.text("微信输入法", "WeType")
        }
    }

    var title: String {
        switch self {
        case .doubao: L10n.text("豆包输入法", "Doubao Input Method")
        case .wetype: L10n.text("微信输入法", "WeType")
        }
    }

    var followsSystemDefaultInput: Bool { self == .wetype }

    var isInstalled: Bool {
        NSWorkspace.shared.urlForApplication(
            withBundleIdentifier: bundleIdentifier
        ) != nil
    }

    /// The stored choice, or the only installed input method when nothing
    /// was chosen yet. Doubao stays the default so existing setups are
    /// unchanged.
    static var current: VoiceInputMethod {
        if let raw = UserDefaults.standard.string(
            forKey: AppStorage.voiceInputMethodKey
        ), let stored = VoiceInputMethod(rawValue: raw) {
            return stored
        }
        if !doubao.isInstalled, wetype.isInstalled { return .wetype }
        return .doubao
    }

    static func select(_ method: VoiceInputMethod) {
        UserDefaults.standard.set(
            method.rawValue,
            forKey: AppStorage.voiceInputMethodKey
        )
    }
}

/// Points the system default input at vRemoteDr 2ch for input methods that
/// follow it, and puts the user's previous default back afterwards.
enum SystemInputRoute {
    private static let targetDeviceName = "vRemoteDr 2ch"
    private static var previousDefaultUID: String?

    static func apply(for method: VoiceInputMethod) {
        if method.followsSystemDefaultInput {
            routeToVRemote()
        } else {
            restore()
        }
    }

    static func routeToVRemote() {
        guard let target = deviceID(named: targetDeviceName) else {
            print("[MIC-ROUTE] \(targetDeviceName) unavailable")
            return
        }
        guard let current = defaultInputDevice() else { return }
        guard current != target else {
            print("[MIC-ROUTE] system input already \(targetDeviceName)")
            return
        }
        if previousDefaultUID == nil {
            previousDefaultUID = uid(of: current)
        }
        let status = setDefaultInputDevice(target)
        print("[MIC-ROUTE] system input -> \(targetDeviceName) status=\(status)")
    }

    /// Restores the default input replaced by `routeToVRemote`, but only
    /// while vRemoteDr 2ch is still the default; a device the user picked
    /// in the meantime is left alone.
    static func restore() {
        guard let uid = previousDefaultUID else { return }
        previousDefaultUID = nil
        guard defaultInputDevice() == deviceID(named: targetDeviceName),
              let previous = deviceID(uid: uid)
        else { return }
        let status = setDefaultInputDevice(previous)
        print("[MIC-ROUTE] system input restored status=\(status)")
    }

    // MARK: - CoreAudio helpers

    private static func address(
        _ selector: AudioObjectPropertySelector
    ) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
    }

    private static func defaultInputDevice() -> AudioDeviceID? {
        var addr = address(kAudioHardwarePropertyDefaultInputDevice)
        var device = AudioDeviceID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioDeviceID>.size)
        let status = AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &addr, 0, nil, &size, &device
        )
        return status == noErr && device != kAudioObjectUnknown
            ? device
            : nil
    }

    private static func setDefaultInputDevice(_ device: AudioDeviceID) -> OSStatus {
        var addr = address(kAudioHardwarePropertyDefaultInputDevice)
        var value = device
        return AudioObjectSetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &addr, 0, nil,
            UInt32(MemoryLayout<AudioDeviceID>.size), &value
        )
    }

    private static func allDevices() -> [AudioDeviceID] {
        var addr = address(kAudioHardwarePropertyDevices)
        var size: UInt32 = 0
        let system = AudioObjectID(kAudioObjectSystemObject)
        guard AudioObjectGetPropertyDataSize(
            system, &addr, 0, nil, &size
        ) == noErr else { return [] }
        var devices = [AudioDeviceID](
            repeating: 0,
            count: Int(size) / MemoryLayout<AudioDeviceID>.size
        )
        guard AudioObjectGetPropertyData(
            system, &addr, 0, nil, &size, &devices
        ) == noErr else { return [] }
        return devices
    }

    private static func stringProperty(
        _ selector: AudioObjectPropertySelector,
        of device: AudioDeviceID
    ) -> String? {
        var addr = address(selector)
        var value: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        guard AudioObjectGetPropertyData(
            device, &addr, 0, nil, &size, &value
        ) == noErr, let value else { return nil }
        return value.takeRetainedValue() as String
    }

    private static func uid(of device: AudioDeviceID) -> String? {
        stringProperty(kAudioDevicePropertyDeviceUID, of: device)
    }

    private static func deviceID(named name: String) -> AudioDeviceID? {
        allDevices().first {
            stringProperty(kAudioObjectPropertyName, of: $0) == name
        }
    }

    private static func deviceID(uid: String) -> AudioDeviceID? {
        allDevices().first { self.uid(of: $0) == uid }
    }
}
