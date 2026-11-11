import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

/// Suppresses the native macOS Search action immediately after X6 emits its
/// Consumer AC Search usage. The gate is armed from the exact VID/PID-matched
/// raw HID callback, so unrelated keyboard events are not globally remapped.
final class X6SearchSuppressor {
    private var tapPort: CFMachPort?
    private var armedUntil = Date.distantPast
    private var triggerKeysDown = Set<Int64>()
    private(set) var isAvailable = false
    var onTriggerDownObserved: ((Bool) -> Void)?
    var onTriggerUpObserved: ((Bool) -> Void)?
    // X6 can deliver the HID report roughly a second before macOS emits the
    // corresponding native Search action. Keep the gate open long enough for
    // that delayed event, while still limiting suppression to the current
    // voice-button gesture.
    private let suppressionDuration: TimeInterval = 1.5

    func start() {
        guard AXIsProcessTrusted() else {
            isAvailable = false
            print("[X6-FILTER] accessibility permission unavailable")
            return
        }
        isAvailable = true

        let mask: CGEventMask =
            (1 << CGEventType.keyDown.rawValue) |
            (1 << CGEventType.keyUp.rawValue) |
            (1 << CGEventType.flagsChanged.rawValue) |
            // NSEvent.systemDefined is CoreGraphics event type 14, but this
            // SDK does not expose it as a CGEventType enum case.
            (1 << 14)
        let callback: CGEventTapCallBack = {
            _, type, event, context -> Unmanaged<CGEvent>?
            in
            guard let context else {
                return Unmanaged.passRetained(event)
            }
            let suppressor = Unmanaged<X6SearchSuppressor>
                .fromOpaque(context)
                .takeUnretainedValue()

            if type == .tapDisabledByTimeout
                || type == .tapDisabledByUserInput
            {
                if let tapPort = suppressor.tapPort {
                    CGEvent.tapEnable(tap: tapPort, enable: true)
                }
                return Unmanaged.passRetained(event)
            }

            let isSynthetic = event.getIntegerValueField(
                .eventSourceUserData
            ) == Key.syntheticMarker

            if isSynthetic {
                print(
                    "[X6-FILTER] delivered synthetic Option " +
                    "type=\(type.rawValue) " +
                    "flags=0x\(String(event.flags.rawValue, radix: 16))"
                )
                return Unmanaged.passRetained(event)
            }

            let keyCode = event.getIntegerValueField(
                .keyboardEventKeycode
            )
            let isArmed = Date() <= suppressor.armedUntil
            let isSearchKey =
                (type == .keyDown || type == .keyUp) && keyCode == 0xB1
            let isSearchSystemEvent = type.rawValue == 14
            let trigger = AppStorage.inputTriggerKey
            let isDelayedX6Trigger = type == .flagsChanged
                && trigger.keyCodes.contains(keyCode)
            if isArmed && (
                isSearchKey || isSearchSystemEvent || isDelayedX6Trigger
            ) {
                print(
                    "[X6-FILTER] suppressed type=\(type.rawValue) " +
                    "keyCode=0x\(String(keyCode, radix: 16))"
                )
                return nil
            }

            suppressor.observeTriggerEvent(
                type: type,
                event: event,
                isSynthetic: false
            )
            return Unmanaged.passRetained(event)
        }

        guard let tap = CGEvent.tapCreate(
            tap: .cghidEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: mask,
            callback: callback,
            userInfo: Unmanaged.passUnretained(self).toOpaque()
        ) else {
            print("[X6-FILTER] unable to create event tap")
            return
        }
        tapPort = tap
        guard let source = CFMachPortCreateRunLoopSource(nil, tap, 0) else {
            print("[X6-FILTER] unable to create run-loop source")
            tapPort = nil
            return
        }
        CFRunLoopAddSource(CFRunLoopGetMain(), source, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
        print("[X6-FILTER] native Search suppression ready")
    }

    func arm() {
        let candidate = Date().addingTimeInterval(suppressionDuration)
        if candidate > armedUntil {
            armedUntil = candidate
        }
    }

    func triggerConfigurationDidChange() {
        triggerKeysDown.removeAll()
        print("[INPUT-TRIGGER] listening for \(AppStorage.inputTriggerKey.title)")
    }

    func stop() {
        armedUntil = .distantPast
        triggerKeysDown.removeAll()
        if let tapPort {
            CGEvent.tapEnable(tap: tapPort, enable: false)
        }
        tapPort = nil
    }

    private func observeTriggerEvent(
        type: CGEventType,
        event: CGEvent,
        isSynthetic: Bool
    ) {
        let keyCode = event.getIntegerValueField(.keyboardEventKeycode)
        // X6-generated Option events are handed to the coordinator directly
        // before posting. Observing them again here would execute the same
        // microphone transition twice.
        guard !isSynthetic else { return }
        let trigger = AppStorage.inputTriggerKey
        guard trigger.keyCodes.contains(keyCode) else { return }

        // macOS normally reports modifier edges as `flagsChanged`, but on
        // some keyboards/input-method paths the DOWN edge arrives as a plain
        // `keyDown` while the release still arrives as `flagsChanged`. V1 only
        // listened to flagsChanged, so Doubao opened but vRemote missed the
        // transition and kept its audio route closed.
        let isDown: Bool
        switch type {
        case .keyDown:
            isDown = true
        case .keyUp:
            isDown = false
        case .flagsChanged:
            // When another Option key remains held, maskAlternate stays set
            // even while this key is released. The per-key set is therefore
            // a more reliable release signal than the aggregate flag alone.
            if triggerKeysDown.contains(keyCode) {
                isDown = false
            } else {
                guard event.flags.contains(trigger.flag) else {
                    print(
                        "[OPTION-RAW] orphan UP ignored keyCode=0x" +
                        String(keyCode, radix: 16)
                    )
                    return
                }
                isDown = true
            }
        default:
            return
        }

        print(
            "[OPTION-RAW] type=\(type.rawValue) " +
            "keyCode=0x\(String(keyCode, radix: 16)) " +
            "edge=\(isDown ? "DOWN" : "UP")"
        )
        if isDown {
            guard triggerKeysDown.insert(keyCode).inserted else { return }
            onTriggerDownObserved?(isSynthetic)
        } else {
            guard triggerKeysDown.remove(keyCode) != nil else { return }
            onTriggerUpObserved?(isSynthetic)
        }
    }
}
