import AppKit
import CoreAudio
import Foundation

/// Reads the input-method's real CoreAudio capture state.
///
/// This is deliberately independent from the remote-button state machine.
/// `kAudioProcessPropertyIsRunningInput` is the source of truth for whether
/// Doubao is currently recording, and `kAudioProcessPropertyDevices` tells us
/// which input device it actually opened.
final class DoubaoAudioStateMonitor {
    enum State: String {
        case unavailable
        case inactive
        case active
    }

    struct Snapshot: Equatable {
        let state: State
        let pid: pid_t?
        let processObjectID: AudioObjectID?
        let inputDeviceIDs: [AudioDeviceID]
        let inputDeviceNames: [String]

        var isRecording: Bool { state == .active }

        var deviceSummary: String {
            inputDeviceNames.isEmpty
                ? "无"
                : inputDeviceNames.joined(separator: "、")
        }
    }

    private static let targetDeviceName = "vRemoteDr 2ch"

    private var started = false
    private var processObjectID = AudioObjectID(kAudioObjectUnknown)
    private var processPID: pid_t?
    private var boundBundleID: String?
    private var lastSnapshot: Snapshot?

    var onSnapshotChanged: ((Snapshot) -> Void)?

    private lazy var processListListener: AudioObjectPropertyListenerBlock = {
        [weak self] _, _ in
        DispatchQueue.main.async {
            self?.refreshBindingAndPublish(force: false)
        }
    }

    private lazy var inputStateListener: AudioObjectPropertyListenerBlock = {
        [weak self] _, _ in
        DispatchQueue.main.async {
            self?.publishCurrentSnapshot(force: false)
        }
    }

    func start() {
        guard !started else { return }
        started = true

        var address = Self.processListAddress
        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            .main,
            processListListener
        )
        if status != noErr {
            print("[DOUBAO-STATE] 无法监听音频进程列表: \(Self.describe(status))")
        }
        refreshBindingAndPublish(force: true)
    }

    /// Rebinds to the newly selected input method's process.
    func targetDidChange() {
        unbindProcessObject()
        boundBundleID = nil
        refreshBindingAndPublish(force: true)
    }

    func stop() {
        guard started else { return }
        started = false
        unbindProcessObject()

        var address = Self.processListAddress
        AudioObjectRemovePropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            .main,
            processListListener
        )
        lastSnapshot = nil
    }

    /// Synchronous pre-event snapshot. The Option event tap calls this before
    /// the key event reaches Doubao, so the returned value is the old state
    /// that the Option press is about to toggle.
    func snapshotNow() -> Snapshot {
        refreshBindingIfNeeded()
        return readCurrentSnapshot()
    }

    private func refreshBindingAndPublish(force: Bool) {
        refreshBindingIfNeeded()
        publishCurrentSnapshot(force: force)
    }

    private func refreshBindingIfNeeded() {
        let targetBundleID = VoiceInputMethod.current.bundleIdentifier
        if boundBundleID != targetBundleID {
            unbindProcessObject()
            boundBundleID = targetBundleID
        }
        let app = NSRunningApplication.runningApplications(
            withBundleIdentifier: targetBundleID
        ).first { !$0.isTerminated }

        guard let app else {
            if processObjectID != kAudioObjectUnknown {
                unbindProcessObject()
            }
            processPID = nil
            return
        }

        let pid = app.processIdentifier
        let translated = Self.processObject(for: pid)
        guard translated != kAudioObjectUnknown else {
            if processObjectID != kAudioObjectUnknown {
                unbindProcessObject()
            }
            processPID = pid
            return
        }

        guard translated != processObjectID else {
            processPID = pid
            return
        }

        unbindProcessObject()
        processObjectID = translated
        processPID = pid

        var runningAddress = Self.runningInputAddress
        let runningStatus = AudioObjectAddPropertyListenerBlock(
            translated,
            &runningAddress,
            .main,
            inputStateListener
        )

        var devicesAddress = Self.inputDevicesAddress
        let devicesStatus = AudioObjectAddPropertyListenerBlock(
            translated,
            &devicesAddress,
            .main,
            inputStateListener
        )

        print(
            "[DOUBAO-STATE] 已绑定 pid=\(pid) audioProcess=\(translated) " +
            "listeners=\(Self.describe(runningStatus))/\(Self.describe(devicesStatus))"
        )
    }

    private func unbindProcessObject() {
        guard processObjectID != kAudioObjectUnknown else { return }
        var runningAddress = Self.runningInputAddress
        AudioObjectRemovePropertyListenerBlock(
            processObjectID,
            &runningAddress,
            .main,
            inputStateListener
        )
        var devicesAddress = Self.inputDevicesAddress
        AudioObjectRemovePropertyListenerBlock(
            processObjectID,
            &devicesAddress,
            .main,
            inputStateListener
        )
        processObjectID = AudioObjectID(kAudioObjectUnknown)
        processPID = nil
    }

    private func publishCurrentSnapshot(force: Bool) {
        guard started else { return }
        let snapshot = readCurrentSnapshot()
        guard force || snapshot != lastSnapshot else { return }
        lastSnapshot = snapshot
        print(
            "[DOUBAO-STATE] state=\(snapshot.state.rawValue) " +
            "pid=\(snapshot.pid.map(String.init) ?? "-") " +
            "devices=\(snapshot.deviceSummary)"
        )
        if snapshot.isRecording,
           !snapshot.inputDeviceNames.contains(Self.targetDeviceName)
        {
            print(
                "[DOUBAO-STATE] ⚠️ \(VoiceInputMethod.current.shortName)" +
                "正在录音，但实际输入不是 " +
                Self.targetDeviceName
            )
        }
        onSnapshotChanged?(snapshot)
    }

    private func readCurrentSnapshot() -> Snapshot {
        guard processObjectID != kAudioObjectUnknown else {
            return Snapshot(
                state: .unavailable,
                pid: processPID,
                processObjectID: nil,
                inputDeviceIDs: [],
                inputDeviceNames: []
            )
        }

        guard let running = Self.uint32Property(
            object: processObjectID,
            selector: kAudioProcessPropertyIsRunningInput,
            scope: kAudioObjectPropertyScopeGlobal
        ) else {
            return Snapshot(
                state: .unavailable,
                pid: processPID,
                processObjectID: processObjectID,
                inputDeviceIDs: [],
                inputDeviceNames: []
            )
        }

        let deviceIDs = Self.objectListProperty(
            object: processObjectID,
            selector: kAudioProcessPropertyDevices,
            scope: kAudioObjectPropertyScopeInput
        )
        let names = deviceIDs.map(Self.deviceName)
        return Snapshot(
            state: running == 0 ? .inactive : .active,
            pid: processPID,
            processObjectID: processObjectID,
            inputDeviceIDs: deviceIDs,
            inputDeviceNames: names
        )
    }

    private static func processObject(for pid: pid_t) -> AudioObjectID {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var qualifier = pid
        var result = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let status = withUnsafePointer(to: &qualifier) { qualifierPointer in
            AudioObjectGetPropertyData(
                AudioObjectID(kAudioObjectSystemObject),
                &address,
                UInt32(MemoryLayout<pid_t>.size),
                qualifierPointer,
                &size,
                &result
            )
        }
        return status == noErr ? result : AudioObjectID(kAudioObjectUnknown)
    }

    private static func uint32Property(
        object: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope
    ) -> UInt32? {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        let status = AudioObjectGetPropertyData(
            object,
            &address,
            0,
            nil,
            &size,
            &value
        )
        return status == noErr ? value : nil
    }

    private static func objectListProperty(
        object: AudioObjectID,
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope
    ) -> [AudioObjectID] {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain
        )
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(
            object,
            &address,
            0,
            nil,
            &size
        ) == noErr, size > 0 else { return [] }

        var values = [AudioObjectID](
            repeating: 0,
            count: Int(size) / MemoryLayout<AudioObjectID>.size
        )
        guard AudioObjectGetPropertyData(
            object,
            &address,
            0,
            nil,
            &size,
            &values
        ) == noErr else { return [] }
        return values
    }

    private static func deviceName(_ deviceID: AudioDeviceID) -> String {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioObjectPropertyName,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: CFString = "未知设备" as CFString
        var size = UInt32(MemoryLayout<CFString>.size)
        let status = withUnsafeMutablePointer(to: &value) { pointer in
            AudioObjectGetPropertyData(
                deviceID,
                &address,
                0,
                nil,
                &size,
                pointer
            )
        }
        return status == noErr ? value as String : "设备 \(deviceID)"
    }

    private static func describe(_ status: OSStatus) -> String {
        status == noErr ? "OK" : String(status)
    }

    private static var processListAddress: AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyProcessObjectList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
    }

    private static var runningInputAddress: AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: kAudioProcessPropertyIsRunningInput,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
    }

    private static var inputDevicesAddress: AudioObjectPropertyAddress {
        AudioObjectPropertyAddress(
            mSelector: kAudioProcessPropertyDevices,
            mScope: kAudioObjectPropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain
        )
    }
}
