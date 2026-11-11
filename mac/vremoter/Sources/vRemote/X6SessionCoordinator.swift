import Foundation

enum VoiceRemoteID: String, CaseIterable {
    case x6
    case chromecast
}

enum RemoteMicrophoneOpenResult: Equatable {
    case sent
    case alreadyStreaming
    case retryAfter(TimeInterval)
    case unavailable
    case failed(String)
}

protocol DoubaoAudioStateProviding: AnyObject {
    var onSnapshotChanged: ((DoubaoAudioStateMonitor.Snapshot) -> Void)? {
        get set
    }

    func start()
    func stop()
    func snapshotNow() -> DoubaoAudioStateMonitor.Snapshot
}

extension DoubaoAudioStateMonitor: DoubaoAudioStateProviding {}

/// Coordinates the input-method toggle, supported remote gestures, and each
/// remote's independent ATVV transport.
///
/// A transport START/STOP is never itself a keyboard toggle. A physical
/// keyboard trigger already reaches Doubao. A remote gesture synthesizes
/// exactly one trigger only when it changes the intended recognition state.
final class X6SessionCoordinator {
    private enum Phase: String {
        case closed
        case opening
        case open
        case closing
    }

    private enum Owner: String {
        case none
        case remote
        case keyboard
    }

    private struct PhysicalVoiceGesture {
        let beganAt: Date
        let recognitionWasOpen: Bool
        let generation: UInt64
    }

    struct DebugSnapshot: Equatable {
        let phase: String
        let generation: UInt64
        let sessionRemote: VoiceRemoteID?
        let streamingRemotes: Set<VoiceRemoteID>
        let openRequestedRemote: VoiceRemoteID?
    }

    private static let remoteHoldThreshold: TimeInterval = 0.55
    private static let openConfirmationTimeout: TimeInterval = 1.0
    // A working Chromecast stream delivers its first frame 150–175 ms
    // after AUDIO_START; a silent ack never does.
    private static let firstAudioFrameTimeout: TimeInterval = 0.3
    private static let closeVerificationTimeout: TimeInterval = 1.2
    private static let maximumOpenAttempts = 3

    private let doubaoState: any DoubaoAudioStateProviding
    private let setRemoteRouteActive: (Bool) -> Void
    private let triggerTap: () -> Void
    private let now: () -> Date

    private var phase: Phase = .closed
    private var owner: Owner = .none
    private var generation: UInt64 = 0
    private var routeActive = false
    private var sessionRemote: VoiceRemoteID?
    private var streamingRemotes = Set<VoiceRemoteID>()
    private var openRequestedRemote: VoiceRemoteID?
    private var openRequestedAttempt = 1
    /// Remote whose silent stream was closed and must reopen once its
    /// AUDIO_STOP arrives, with the attempt number to use.
    private var reopenAfterStop: (remote: VoiceRemoteID, attempt: Int)?
    private var physicalVoiceGestures = [VoiceRemoteID: PhysicalVoiceGesture]()

    private var routeCloseWorkItem: DispatchWorkItem?
    private var startVerificationWorkItem: DispatchWorkItem?
    private var closeVerificationWorkItem: DispatchWorkItem?
    private var openRetryWorkItem: DispatchWorkItem?
    private var openConfirmationWorkItem: DispatchWorkItem?
    private var firstFrameWorkItem: DispatchWorkItem?
    private var firstFrameRemote: VoiceRemoteID?

    var preferredRemoteProvider: (() -> VoiceRemoteID?)?
    var onMicrophoneOpenRequested:
        ((VoiceRemoteID, Bool) -> RemoteMicrophoneOpenResult)?
    var onMicrophoneCloseRequested: (() -> Void)?
    var onRemoteMicrophoneCloseRequested: ((VoiceRemoteID) -> Void)?
    var onStateChanged: ((String) -> Void)?

    init(
        doubaoState: any DoubaoAudioStateProviding,
        setRemoteRouteActive: @escaping (Bool) -> Void = {
            AudioPipe.shared.setRemoteActive($0)
        },
        triggerTap: @escaping () -> Void = { Key.triggerTap() },
        now: @escaping () -> Date = Date.init
    ) {
        self.doubaoState = doubaoState
        self.setRemoteRouteActive = setRemoteRouteActive
        self.triggerTap = triggerTap
        self.now = now
    }

    var debugSnapshot: DebugSnapshot {
        DebugSnapshot(
            phase: phase.rawValue,
            generation: generation,
            sessionRemote: sessionRemote,
            streamingRemotes: streamingRemotes,
            openRequestedRemote: openRequestedRemote
        )
    }

    func start() {
        doubaoState.onSnapshotChanged = { [weak self] snapshot in
            self?.handleDoubaoSnapshot(snapshot)
        }
        doubaoState.start()
    }

    // MARK: - ATVV transport

    func remoteAudioStarted(
        remote: VoiceRemoteID,
        reason: UInt8,
        supportsPhysicalHoldGesture: Bool = false
    ) {
        let snapshot = doubaoState.snapshotNow()
        let inserted = streamingRemotes.insert(remote).inserted
        let hostRequestedAttempt =
            openRequestedRemote == remote ? openRequestedAttempt : nil
        if openRequestedRemote == remote {
            openRequestedRemote = nil
            openConfirmationWorkItem?.cancel()
            openConfirmationWorkItem = nil
        }

        if phase == .closing {
            print(
                "[VOICE-SESSION] late AUDIO_START while closing " +
                "remote=\(remote.rawValue) reason=" +
                String(format: "0x%02x", reason)
            )
            closeAllRemoteTransports()
            return
        }

        if supportsPhysicalHoldGesture, reason == 0x03 {
            let recognitionWasOpen = phase == .open || snapshot.isRecording
            if physicalVoiceGestures[remote] == nil {
                physicalVoiceGestures[remote] = PhysicalVoiceGesture(
                    beganAt: now(),
                    recognitionWasOpen: recognitionWasOpen,
                    generation: generation
                )
            }
            print(
                "[VOICE-SESSION] physical ATVV DOWN " +
                "remote=\(remote.rawValue) " +
                "recognitionWasOpen=\(recognitionWasOpen)"
            )

            if recognitionWasOpen {
                sessionRemote = remote
                activateRemoteRoute()
                publish("遥控器按住录音中")
                return
            }
        }

        if !inserted {
            print(
                "[VOICE-SESSION] duplicate AUDIO_START ignored " +
                "remote=\(remote.rawValue) reason=" +
                String(format: "0x%02x", reason)
            )
            return
        }

        activateRemoteRoute()
        print(
            "[VOICE-SESSION] AUDIO_START remote=\(remote.rawValue) reason=" +
            String(format: "0x%02x", reason) +
            " phase=\(phase.rawValue) owner=\(owner.rawValue) " +
            "doubaoBefore=\(snapshot.state.rawValue) " +
            "device=\(snapshot.deviceSummary)"
        )

        switch phase {
        case .opening, .open:
            publish("豆包录音中 · 遥控器已接入")
            // After a very short Chromecast tap the remote can ack MIC_OPEN
            // with AUDIO_START yet never stream audio (mic LED stays off).
            if remote == .chromecast, reason == 0x00,
               let attempt = hostRequestedAttempt
            {
                scheduleFirstFrameWatchdog(remote: remote, attempt: attempt)
            }

        case .closed:
            // Chromecast reserves reason 0x03 for a physical voice press, so
            // a reason-0 stream while closed can only be a stale response to
            // an earlier host request. X6 is different: its verified physical
            // first press itself arrives as reason 0x00 and must remain a
            // valid open gesture.
            guard remote != .chromecast || reason != 0x00 else {
                print(
                    "[VOICE-SESSION] stale host AUDIO_START ignored " +
                    "remote=\(remote.rawValue)"
                )
                closeAllRemoteTransports()
                return
            }
            beginOpening(
                owner: .remote,
                remote: remote,
                sendTrigger: true,
                requestTransportImmediately: false
            )
            // beginOpening advances the session generation. Keep the
            // in-flight physical gesture attached to that new session so its
            // matching AUDIO_STOP can complete the tap/hold decision.
            if supportsPhysicalHoldGesture,
               reason == 0x03,
               let gesture = physicalVoiceGestures[remote]
            {
                physicalVoiceGestures[remote] = PhysicalVoiceGesture(
                    beganAt: gesture.beganAt,
                    recognitionWasOpen: gesture.recognitionWasOpen,
                    generation: generation
                )
            }

        case .closing:
            break
        }
    }

    func remoteAudioFrameReceived(remote: VoiceRemoteID) {
        cancelFirstFrameWatchdog(for: remote)
    }

    func remoteAudioStopped(remote: VoiceRemoteID, reason: UInt8) {
        cancelFirstFrameWatchdog(for: remote)
        streamingRemotes.remove(remote)
        if openRequestedRemote == remote {
            openRequestedRemote = nil
            openConfirmationWorkItem?.cancel()
            openConfirmationWorkItem = nil
        }

        if reason == 0x02,
           let gesture = physicalVoiceGestures.removeValue(forKey: remote)
        {
            let duration = now().timeIntervalSince(gesture.beganAt)
            let wasHold = duration >= Self.remoteHoldThreshold
            print(
                String(
                    format:
                        "[VOICE-SESSION] physical ATVV UP remote=%@ " +
                        "duration=%.3fs kind=%@ recognitionWasOpen=%@",
                    remote.rawValue,
                    duration,
                    wasHold ? "HOLD" : "TAP",
                    gesture.recognitionWasOpen ? "true" : "false"
                )
            )

            guard gesture.generation == generation else {
                print(
                    "[VOICE-SESSION] stale physical release ignored " +
                    "remote=\(remote.rawValue)"
                )
                return
            }

            if wasHold || gesture.recognitionWasOpen {
                closeRecognitionFromRemote(
                    reason: wasHold
                        ? "ATVV HOLD release"
                        : "ATVV TAP toggle"
                )
                return
            }

            // First Chromecast short tap: the physical stream ends at button
            // release. Immediately request a persistent stream from that same
            // remote. BLEBridge has already marked its local stream idle
            // before invoking this callback, so no fixed delay is necessary.
            guard phase == .opening || phase == .open else { return }
            sessionRemote = remote
            requestRemoteTransportOpen(
                remote,
                bypassDebounce: true,
                attempt: 1
            )
            publish("豆包录音中 · 遥控器持续收音")
            return
        }

        print(
            "[VOICE-SESSION] AUDIO_STOP remote=\(remote.rawValue) reason=" +
            String(format: "0x%02x", reason) +
            " phase=\(phase.rawValue)"
        )

        if let pending = reopenAfterStop, pending.remote == remote {
            reopenAfterStop = nil
            guard phase == .opening || phase == .open else { return }
            requestRemoteTransportOpen(
                remote,
                bypassDebounce: true,
                attempt: pending.attempt
            )
            return
        }

        // Do not automatically reopen a generic transport STOP. X6 may emit
        // it immediately before its HID toggle-off edge. Only the proven
        // Chromecast physical-tap path above performs an immediate recovery.
        if phase == .open || phase == .opening {
            publish("豆包仍开启 · 遥控器音频已停止")
        } else if streamingRemotes.isEmpty {
            scheduleRouteClose(after: 0.08)
        }
    }

    func remoteDisconnected(_ remote: VoiceRemoteID) {
        streamingRemotes.remove(remote)
        physicalVoiceGestures.removeValue(forKey: remote)
        if openRequestedRemote == remote {
            openRequestedRemote = nil
            openRetryWorkItem?.cancel()
            openRetryWorkItem = nil
            openConfirmationWorkItem?.cancel()
            openConfirmationWorkItem = nil
        }

        guard sessionRemote == remote else { return }
        sessionRemote = preferredRemoteProvider?()
        guard phase == .opening || phase == .open,
              let replacement = sessionRemote
        else { return }
        requestRemoteTransportOpen(
            replacement,
            bypassDebounce: false,
            attempt: 1
        )
    }

    func remoteMicrophoneOpenFailed(remote: VoiceRemoteID, code: UInt16) {
        guard openRequestedRemote == remote,
              phase == .opening || phase == .open
        else { return }
        openRequestedRemote = nil
        openConfirmationWorkItem?.cancel()
        openConfirmationWorkItem = nil
        print(
            "[VOICE-SESSION] MIC_OPEN failed remote=\(remote.rawValue) " +
            String(format: "code=0x%04x", code)
        )
        scheduleOpenRetry(remote: remote, after: 0.20, attempt: 2)
    }

    // MARK: - X6 HID completion

    func remoteHIDShortPress() {
        closeRecognitionFromRemote(reason: "HID SHORT")
    }

    func remoteHIDLongPressEnded() {
        closeRecognitionFromRemote(reason: "HID LONG release")
    }

    // MARK: - Physical keyboard trigger

    /// Called before the real physical trigger reaches Doubao. It updates the
    /// remote transport only; it never synthesizes a second keyboard event.
    func triggerDownObserved(isSynthetic: Bool) {
        guard !isSynthetic else { return }
        let snapshot = doubaoState.snapshotNow()
        let shouldOpen: Bool
        switch phase {
        case .closed:
            shouldOpen = !snapshot.isRecording
        case .closing:
            // A second deliberate physical trigger reverses the user's last
            // intent even if CoreAudio has not published inactive yet.
            shouldOpen = true
        case .opening, .open:
            shouldOpen = false
        }

        print(
            "[TRIGGER] DOWN source=keyboard action=" +
            (shouldOpen ? "open" : "close") +
            " phase=\(phase.rawValue) " +
            "doubaoBefore=\(snapshot.state.rawValue) " +
            "device=\(snapshot.deviceSummary)"
        )

        if shouldOpen {
            guard let remote = preferredRemoteProvider?() else {
                beginOpeningWithoutRemote()
                return
            }
            beginOpening(
                owner: .keyboard,
                remote: remote,
                sendTrigger: false,
                requestTransportImmediately: true
            )
        } else {
            beginClosing(sendTrigger: false, reason: "keyboard trigger")
        }
    }

    func triggerUpObserved(isSynthetic: Bool) {
        guard !isSynthetic else { return }
        print(
            "[TRIGGER] UP source=keyboard phase=\(phase.rawValue) " +
            "owner=\(owner.rawValue)"
        )
        reconcileSoon(after: 0.03)
        reconcileSoon(after: 0.15)
    }

    // MARK: - Lifecycle / manual control

    func forceClose() {
        generation &+= 1
        cancelScheduledWork()
        phase = .closed
        owner = .none
        sessionRemote = nil
        physicalVoiceGestures.removeAll()
        closeRouteAndTransports()
        publish("已手动关闭")
    }

    func stop() {
        forceClose()
        doubaoState.stop()
    }

    // MARK: - Doubao observation

    private func handleDoubaoSnapshot(
        _ snapshot: DoubaoAudioStateMonitor.Snapshot
    ) {
        switch snapshot.state {
        case .active:
            switch phase {
            case .opening:
                startVerificationWorkItem?.cancel()
                startVerificationWorkItem = nil
                phase = .open
                activateRemoteRoute()
                if let remote = sessionRemote,
                   !streamingRemotes.contains(remote),
                   openRequestedRemote != remote
                {
                    requestRemoteTransportOpen(
                        remote,
                        bypassDebounce: false,
                        attempt: 1
                    )
                }
                publish("豆包录音中 · \(snapshot.deviceSummary)")

            case .open:
                activateRemoteRoute()
                publish("豆包录音中 · \(snapshot.deviceSummary)")

            case .closing:
                // CoreAudio can remain active briefly after the trigger has
                // toggled Doubao off. Never reinterpret this stale edge as a
                // fresh external opening.
                print(
                    "[VOICE-SESSION] active snapshot ignored while closing"
                )

            case .closed:
                adoptExternalActivation(snapshot)
            }

        case .inactive:
            switch phase {
            case .opening:
                // The opening verification owns this transient idle window.
                break
            case .open, .closing:
                finalizeClosed(reason: "Doubao inactive")
            case .closed:
                if routeActive || !streamingRemotes.isEmpty {
                    closeRouteAndTransports()
                }
            }
            publish("豆包已就绪")

        case .unavailable:
            if phase != .opening {
                finalizeClosed(reason: "Doubao unavailable")
            }
            publish("等待豆包输入法")
        }
    }

    private func adoptExternalActivation(
        _ snapshot: DoubaoAudioStateMonitor.Snapshot
    ) {
        generation &+= 1
        cancelScheduledWork()
        phase = .open
        owner = .keyboard
        sessionRemote = preferredRemoteProvider?()
        activateRemoteRoute()
        print(
            "[VOICE-SESSION] Doubao external activation adopted " +
            "device=\(snapshot.deviceSummary)"
        )
        if let remote = sessionRemote {
            requestRemoteTransportOpen(
                remote,
                bypassDebounce: false,
                attempt: 1
            )
        }
        publish("豆包录音中 · \(snapshot.deviceSummary)")
    }

    // MARK: - State transitions

    private func beginOpeningWithoutRemote() {
        generation &+= 1
        cancelScheduledWork()
        phase = .opening
        owner = .keyboard
        sessionRemote = nil
        activateRemoteRoute()
        scheduleStartVerification(for: generation)
        publish("豆包正在启动 · 等待遥控器")
    }

    private func beginOpening(
        owner: Owner,
        remote: VoiceRemoteID,
        sendTrigger: Bool,
        requestTransportImmediately: Bool
    ) {
        generation &+= 1
        cancelScheduledWork()
        phase = .opening
        self.owner = owner
        sessionRemote = remote
        activateRemoteRoute()
        if sendTrigger {
            triggerTap()
        }
        if requestTransportImmediately {
            requestRemoteTransportOpen(
                remote,
                bypassDebounce: false,
                attempt: 1
            )
        }
        scheduleStartVerification(for: generation)
        publish(
            owner == .keyboard
                ? "豆包正在启动 · 遥控器已预热"
                : "遥控器已启动 · 正在打开豆包"
        )
    }

    private func closeRecognitionFromRemote(reason: String) {
        guard phase == .opening || phase == .open else {
            print(
                "[VOICE-SESSION] \(reason) ignored phase=\(phase.rawValue)"
            )
            return
        }
        beginClosing(sendTrigger: true, reason: reason)
    }

    private func beginClosing(sendTrigger: Bool, reason: String) {
        guard phase != .closed, phase != .closing else {
            print(
                "[VOICE-SESSION] close ignored phase=\(phase.rawValue) " +
                "reason=\(reason)"
            )
            return
        }

        generation &+= 1
        cancelScheduledWork()
        phase = .closing
        owner = .none
        openRequestedRemote = nil
        if sendTrigger {
            triggerTap()
        }
        scheduleRouteClose(after: 0.10)
        scheduleCloseVerification(for: generation)
        publish("遥控器已结束 · 正在关闭豆包")
        print(
            "[VOICE-SESSION] begin closing reason=\(reason) " +
            "trigger=\(sendTrigger ? "synthetic" : "physical")"
        )
    }

    private func finalizeClosed(reason: String) {
        generation &+= 1
        cancelScheduledWork()
        phase = .closed
        owner = .none
        sessionRemote = nil
        physicalVoiceGestures.removeAll()
        closeRouteAndTransports()
        print("[VOICE-SESSION] closed reason=\(reason)")
    }

    // MARK: - Remote transport requests

    private func requestRemoteTransportOpen(
        _ remote: VoiceRemoteID,
        bypassDebounce: Bool,
        attempt: Int
    ) {
        guard phase == .opening || phase == .open,
              !streamingRemotes.contains(remote),
              openRequestedRemote != remote
        else { return }

        guard let request = onMicrophoneOpenRequested else {
            print(
                "[VOICE-SESSION] MIC_OPEN unavailable " +
                "remote=\(remote.rawValue)"
            )
            return
        }

        let result = request(remote, bypassDebounce)
        switch result {
        case .sent:
            openRequestedRemote = remote
            openRequestedAttempt = attempt
            scheduleOpenConfirmation(
                remote: remote,
                attempt: attempt,
                generation: generation
            )
            print(
                "[VOICE-SESSION] MIC_OPEN sent remote=\(remote.rawValue) " +
                "attempt=\(attempt)"
            )

        case .alreadyStreaming:
            streamingRemotes.insert(remote)
            openRequestedRemote = nil
            print(
                "[VOICE-SESSION] MIC_OPEN already streaming " +
                "remote=\(remote.rawValue)"
            )

        case .retryAfter(let delay):
            scheduleOpenRetry(
                remote: remote,
                after: delay,
                attempt: attempt + 1
            )

        case .unavailable:
            print(
                "[VOICE-SESSION] MIC_OPEN target unavailable " +
                "remote=\(remote.rawValue)"
            )

        case .failed(let message):
            print(
                "[VOICE-SESSION] MIC_OPEN failed remote=\(remote.rawValue) " +
                "message=\(message)"
            )
            scheduleOpenRetry(
                remote: remote,
                after: 0.20,
                attempt: attempt + 1
            )
        }
    }

    private func scheduleOpenRetry(
        remote: VoiceRemoteID,
        after delay: TimeInterval,
        attempt: Int
    ) {
        guard attempt <= Self.maximumOpenAttempts else {
            print(
                "[VOICE-SESSION] MIC_OPEN retries exhausted " +
                "remote=\(remote.rawValue)"
            )
            return
        }
        openRetryWorkItem?.cancel()
        let expectedGeneration = generation
        let work = DispatchWorkItem { [weak self] in
            guard let self,
                  self.generation == expectedGeneration,
                  self.phase == .opening || self.phase == .open
            else { return }
            self.openRetryWorkItem = nil
            self.requestRemoteTransportOpen(
                remote,
                bypassDebounce: true,
                attempt: attempt
            )
        }
        openRetryWorkItem = work
        DispatchQueue.main.asyncAfter(
            deadline: .now() + max(0.01, delay),
            execute: work
        )
    }

    private func scheduleOpenConfirmation(
        remote: VoiceRemoteID,
        attempt: Int,
        generation expectedGeneration: UInt64
    ) {
        openConfirmationWorkItem?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self,
                  self.generation == expectedGeneration,
                  self.openRequestedRemote == remote,
                  !self.streamingRemotes.contains(remote),
                  self.phase == .opening || self.phase == .open
            else { return }
            self.openRequestedRemote = nil
            self.openConfirmationWorkItem = nil
            print(
                "[VOICE-SESSION] MIC_OPEN confirmation timeout " +
                "remote=\(remote.rawValue) attempt=\(attempt)"
            )
            self.scheduleOpenRetry(
                remote: remote,
                after: 0.05,
                attempt: attempt + 1
            )
        }
        openConfirmationWorkItem = work
        DispatchQueue.main.asyncAfter(
            deadline: .now() + Self.openConfirmationTimeout,
            execute: work
        )
    }

    /// Closes and reopens a stream the remote acknowledged but never fed
    /// with audio. The reopen waits for the remote's AUDIO_STOP so the
    /// bridge is idle again before the next MIC_OPEN.
    private func scheduleFirstFrameWatchdog(
        remote: VoiceRemoteID,
        attempt: Int
    ) {
        firstFrameWorkItem?.cancel()
        firstFrameRemote = remote
        let expectedGeneration = generation
        let work = DispatchWorkItem { [weak self] in
            guard let self,
                  self.generation == expectedGeneration,
                  self.streamingRemotes.contains(remote),
                  self.phase == .opening || self.phase == .open
            else { return }
            self.firstFrameWorkItem = nil
            self.firstFrameRemote = nil
            guard attempt < Self.maximumOpenAttempts else {
                print(
                    "[VOICE-SESSION] MIC_OPEN acked but no audio; " +
                    "retries exhausted remote=\(remote.rawValue)"
                )
                return
            }
            print(
                "[VOICE-SESSION] MIC_OPEN acked but no audio; reopening " +
                "remote=\(remote.rawValue) attempt=\(attempt + 1)"
            )
            self.reopenAfterStop = (remote, attempt + 1)
            self.onRemoteMicrophoneCloseRequested?(remote)
        }
        firstFrameWorkItem = work
        DispatchQueue.main.asyncAfter(
            deadline: .now() + Self.firstAudioFrameTimeout,
            execute: work
        )
    }

    private func cancelFirstFrameWatchdog(for remote: VoiceRemoteID) {
        guard firstFrameRemote == remote else { return }
        firstFrameWorkItem?.cancel()
        firstFrameWorkItem = nil
        firstFrameRemote = nil
    }

    // MARK: - Verification and route lifecycle

    private func scheduleStartVerification(for expectedGeneration: UInt64) {
        startVerificationWorkItem?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self,
                  self.generation == expectedGeneration,
                  self.phase == .opening
            else { return }
            self.startVerificationWorkItem = nil
            let snapshot = self.doubaoState.snapshotNow()
            if snapshot.isRecording {
                self.handleDoubaoSnapshot(snapshot)
                return
            }
            self.finalizeClosed(reason: "opening timeout")
            self.publish("豆包未启动 · 遥控器自动关闭")
        }
        startVerificationWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0, execute: work)
    }

    private func scheduleCloseVerification(for expectedGeneration: UInt64) {
        closeVerificationWorkItem?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self,
                  self.generation == expectedGeneration,
                  self.phase == .closing
            else { return }
            self.closeVerificationWorkItem = nil
            let snapshot = self.doubaoState.snapshotNow()
            if snapshot.isRecording {
                // The requested close did not become observable. Follow the
                // real CoreAudio state without synthesizing another trigger.
                self.phase = .open
                self.owner = .keyboard
                self.sessionRemote = self.preferredRemoteProvider?()
                self.activateRemoteRoute()
                if let remote = self.sessionRemote {
                    self.requestRemoteTransportOpen(
                        remote,
                        bypassDebounce: false,
                        attempt: 1
                    )
                }
                self.publish("豆包仍在录音 · 请再次关闭")
                print(
                    "[VOICE-SESSION] close not observed; restored open state"
                )
            } else {
                self.finalizeClosed(reason: "close verification")
                self.publish("豆包已就绪")
            }
        }
        closeVerificationWorkItem = work
        DispatchQueue.main.asyncAfter(
            deadline: .now() + Self.closeVerificationTimeout,
            execute: work
        )
    }

    private func reconcileSoon(after delay: TimeInterval) {
        let expectedGeneration = generation
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
            guard let self, self.generation == expectedGeneration else { return }
            self.handleDoubaoSnapshot(self.doubaoState.snapshotNow())
        }
    }

    private func activateRemoteRoute() {
        routeCloseWorkItem?.cancel()
        routeCloseWorkItem = nil
        guard !routeActive else { return }
        routeActive = true
        setRemoteRouteActive(true)
        print(
            "[VOICE-SESSION] audio route ON phase=\(phase.rawValue) " +
            "owner=\(owner.rawValue)"
        )
    }

    private func scheduleRouteClose(after delay: TimeInterval) {
        routeCloseWorkItem?.cancel()
        let expectedGeneration = generation
        let work = DispatchWorkItem { [weak self] in
            guard let self, self.generation == expectedGeneration else { return }
            self.routeCloseWorkItem = nil
            self.closeRouteAndTransports()
        }
        routeCloseWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    private func closeAllRemoteTransports() {
        onMicrophoneCloseRequested?()
        streamingRemotes.removeAll()
        openRequestedRemote = nil
        openRetryWorkItem?.cancel()
        openRetryWorkItem = nil
        openConfirmationWorkItem?.cancel()
        openConfirmationWorkItem = nil
        firstFrameWorkItem?.cancel()
        firstFrameWorkItem = nil
        firstFrameRemote = nil
        reopenAfterStop = nil
    }

    private func closeRouteAndTransports() {
        let hadTransport =
            !streamingRemotes.isEmpty || openRequestedRemote != nil
        routeActive = false
        setRemoteRouteActive(false)
        if hadTransport {
            closeAllRemoteTransports()
        }
        print("[VOICE-SESSION] audio route OFF")
    }

    private func cancelScheduledWork() {
        routeCloseWorkItem?.cancel()
        routeCloseWorkItem = nil
        startVerificationWorkItem?.cancel()
        startVerificationWorkItem = nil
        closeVerificationWorkItem?.cancel()
        closeVerificationWorkItem = nil
        openRetryWorkItem?.cancel()
        openRetryWorkItem = nil
        openConfirmationWorkItem?.cancel()
        openConfirmationWorkItem = nil
        firstFrameWorkItem?.cancel()
        firstFrameWorkItem = nil
        firstFrameRemote = nil
        reopenAfterStop = nil
    }

    private func publish(_ status: String) {
        // Status copy is written for Doubao; name the selected input method.
        let method = VoiceInputMethod.current
        onStateChanged?(
            status
                .replacingOccurrences(of: "豆包输入法", with: method.title)
                .replacingOccurrences(of: "豆包", with: method.shortName)
        )
    }
}
