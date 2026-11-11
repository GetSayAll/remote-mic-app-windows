import Foundation
import IOKit.hid

private func chromecastRemoteMatched(
    context: UnsafeMutableRawPointer?,
    result: IOReturn,
    sender: UnsafeMutableRawPointer?,
    device: IOHIDDevice
) {
    guard let context else { return }
    Unmanaged<ChromecastRemoteHIDBridge>
        .fromOpaque(context)
        .takeUnretainedValue()
        .deviceDidMatch(result: result, device: device)
}

private func chromecastRemoteRemoved(
    context: UnsafeMutableRawPointer?,
    result: IOReturn,
    sender: UnsafeMutableRawPointer?,
    device: IOHIDDevice
) {
    guard let context else { return }
    Unmanaged<ChromecastRemoteHIDBridge>
        .fromOpaque(context)
        .takeUnretainedValue()
        .deviceDidRemove(device)
}

private func chromecastRemoteInputReport(
    context: UnsafeMutableRawPointer?,
    result: IOReturn,
    sender: UnsafeMutableRawPointer?,
    type: IOHIDReportType,
    reportID: UInt32,
    report: UnsafeMutablePointer<UInt8>,
    reportLength: CFIndex
) {
    guard let context, result == kIOReturnSuccess, reportLength > 0 else {
        return
    }
    Unmanaged<ChromecastRemoteHIDBridge>
        .fromOpaque(context)
        .takeUnretainedValue()
        .handleReport(
            reportID: reportID,
            data: Data(bytes: report, count: reportLength)
        )
}

/// Connection monitor for the Google Chromecast Remote HID interface.
///
/// Its voice key does not expose a dependable HID key-down usage on macOS;
/// voice gestures are therefore handled by the remote's ATVV control stream.
final class ChromecastRemoteHIDBridge {
    static let vendorID = 0x18D1
    static let productID = 0x9450

    var onConnectionChanged: ((Bool) -> Void)?

    private var manager: IOHIDManager?
    private var activeDevice: IOHIDDevice?
    private var remappingEnabled = RemoteMappingStore.shared.isEnabled(.chromecast)
    private var lastButtonID: String?

    func start() {
        stop()
        let manager = IOHIDManagerCreate(
            kCFAllocatorDefault,
            IOOptionBits(kIOHIDOptionsTypeNone)
        )
        IOHIDManagerSetDeviceMatching(
            manager,
            [
                kIOHIDVendorIDKey: Self.vendorID,
                kIOHIDProductIDKey: Self.productID,
            ] as CFDictionary
        )

        let context = Unmanaged.passUnretained(self).toOpaque()
        IOHIDManagerRegisterDeviceMatchingCallback(
            manager,
            chromecastRemoteMatched,
            context
        )
        IOHIDManagerRegisterDeviceRemovalCallback(
            manager,
            chromecastRemoteRemoved,
            context
        )
        IOHIDManagerRegisterInputReportCallback(
            manager,
            chromecastRemoteInputReport,
            context
        )
        IOHIDManagerScheduleWithRunLoop(
            manager,
            CFRunLoopGetMain(),
            CFRunLoopMode.commonModes.rawValue
        )
        // Open the manager exclusively, not just one IOHIDDevice returned by
        // the match callback. The remote exposes more than one HID
        // collection; seizing only the first collection still lets its media
        // usage reach macOS (for example Select can launch Apple Music).
        let openOptions = remappingEnabled
            ? IOOptionBits(kIOHIDOptionsTypeSeizeDevice)
            : IOOptionBits(kIOHIDOptionsTypeNone)
        var result = IOHIDManagerOpen(manager, openOptions)
        if result == kIOReturnNotPrivileged, remappingEnabled {
            // macOS only lets privileged clients seize HID devices whose
            // primary usage is a keyboard. Keep mapping alive in shared mode;
            // the remote's native actions are then no longer suppressed.
            print(
                "[CAST-HID] seize not permitted; " +
                "falling back to shared mode (native actions not suppressed)"
            )
            result = IOHIDManagerOpen(
                manager,
                IOOptionBits(kIOHIDOptionsTypeNone)
            )
        }
        guard result == kIOReturnSuccess else {
            print(
                "[CAST-HID] manager open failed: " +
                String(format: "0x%08X", UInt32(bitPattern: result))
            )
            IOHIDManagerUnscheduleFromRunLoop(
                manager,
                CFRunLoopGetMain(),
                CFRunLoopMode.commonModes.rawValue
            )
            return
        }
        self.manager = manager
        print("[CAST-HID] waiting for Chromecast Remote")
    }

    func stop() {
        activeDevice = nil
        lastButtonID = nil
        onConnectionChanged?(false)

        guard let manager else { return }
        IOHIDManagerUnscheduleFromRunLoop(
            manager,
            CFRunLoopGetMain(),
            CFRunLoopMode.commonModes.rawValue
        )
        IOHIDManagerClose(manager, IOOptionBits(kIOHIDOptionsTypeNone))
        self.manager = nil
    }

    func setRemappingEnabled(_ enabled: Bool) {
        guard remappingEnabled != enabled else { return }
        remappingEnabled = enabled
        start()
    }

    fileprivate func deviceDidMatch(
        result: IOReturn,
        device: IOHIDDevice
    ) {
        guard result == kIOReturnSuccess, activeDevice == nil else { return }
        // IOHIDManagerOpen already opened every current and future matching
        // HID collection with the requested access mode.
        activeDevice = device
        onConnectionChanged?(true)
        print(
            "[CAST-HID] connected VID=0x18d1 PID=0x9450 " +
            "mode=\(remappingEnabled ? "remap" : "observe")"
        )
    }

    fileprivate func deviceDidRemove(_ device: IOHIDDevice) {
        guard let activeDevice, CFEqual(activeDevice, device) else { return }
        self.activeDevice = nil
        onConnectionChanged?(false)
        print("[CAST-HID] disconnected")
    }

    fileprivate func handleReport(reportID: UInt32, data: Data) {
        guard reportID == 0x01 else { return }
        let payload = data.first == UInt8(truncatingIfNeeded: reportID)
            ? Data(data.dropFirst())
            : data
        guard let usage = payload.first else { return }

        if usage == 0 {
            guard let buttonID = lastButtonID else { return }
            lastButtonID = nil
            apply(buttonID: buttonID, isDown: false)
            return
        }

        let buttonID = String(format: "%02X", usage)
        guard lastButtonID != buttonID else { return }
        if let previous = lastButtonID {
            apply(buttonID: previous, isDown: false)
        }
        lastButtonID = buttonID
        apply(buttonID: buttonID, isDown: true)
    }

    private func apply(buttonID: String, isDown: Bool) {
        guard remappingEnabled,
              let button = RemoteProfiles.chromecastButtons.first(
                where: { $0.id == buttonID }
              )
        else { return }
        let store = RemoteMappingStore.shared
        store.post(button: button, remote: .chromecast, isDown: isDown)
        print(
            "[CAST-MAP] \(button.title) " +
            "\(isDown ? "DOWN" : "UP") -> " +
            store.targetTitle(for: button, remote: .chromecast)
        )
    }
}
