// BLEBridge: CoreBluetooth connection to one ATVV-capable voice remote. It
// runs the voice-stream handshake and pumps decoded mono Int16 PCM into the
// AudioPipe.
//
// State machine (matches mi-ao's ATVV flow, simplified):
//   disconnected → connect → discoverServices → discoverChars
//   → send getCapabilities → wait for capabilities
//   → on .startSearch (voice-key press from remote): send micOpen
//   → audio frames flow on TX char → ADPCM decode → AudioPipe.feed

import CoreBluetooth
import Foundation

final class BLEBridge: NSObject {
    // ATVV service / characteristic UUIDs (Google Android TV Remote protocol).
    static let serviceUUID  = CBUUID(string: "AB5E0001-5A21-4F05-BC7D-AF01F617B664")
    static let commandUUID  = CBUUID(string: "AB5E0002-5A21-4F05-BC7D-AF01F617B664")  // host writes commands to remote
    static let audioUUID    = CBUUID(string: "AB5E0003-5A21-4F05-BC7D-AF01F617B664")  // remote sends voice data to host (notify)
    static let controlUUID  = CBUUID(string: "AB5E0004-5A21-4F05-BC7D-AF01F617B664")  // control events from remote

    // This V2 bridge owns one X6 ATVV connection.
    private let nameHint: String?
    private let savedUUIDPath: String
    private let recordingPrefix: String
    private let logTag: String
    private let resetSessionOnConnect: Bool

    /// Where we persist a previously-discovered peripheral UUID so the next
    /// launch can attempt `retrievePeripherals` directly without scanning.
    private static let appSupportDirectory = NSString(
        string: "~/Library/Application Support/vRemote"
    ).expandingTildeInPath

    /// Called whenever the bridge produces or stops producing audio (true = streaming).
    var onStreamingChanged: ((Bool, Int /* sampleRate */) -> Void)?
    /// Raw ATVV session edges. Unlike `onStreamingChanged`, these preserve
    /// the protocol reason so X6 can distinguish a native hold-to-talk stream
    /// from a host-requested persistent stream.
    var onAudioStarted: ((UInt8 /* reason */, Int /* sampleRate */) -> Void)?
    var onAudioStopped: ((UInt8 /* reason */) -> Void)?
    /// First decoded audio frame of the current stream. An AUDIO_START ack
    /// alone does not prove the remote's microphone is actually capturing.
    var onFirstAudioFrame: (() -> Void)?
    var onMicrophoneOpenFailed: ((UInt16 /* code */) -> Void)?
    /// Called when the device connection state changes.
    var onConnectionChanged: ((Bool) -> Void)?
    /// Audio level meter update; arg is dBFS in [-60, 0].
    var onLevel: ((Double, Int /* peak */) -> Void)?

    private var central: CBCentralManager!
    private var isStarted = false
    private var peripheral: CBPeripheral?
    private var audioChar: CBCharacteristic?
    private var commandChar: CBCharacteristic?
    private var controlChar: CBCharacteristic?
    private var atvvCharsComplete = false
    private var connectedPeripheralID: UUID?
    private var connectTimeout: DispatchWorkItem?
    /// Polls for the remote while scanning. A remote that macOS has already
    /// reconnected as HID stops advertising, so the scan alone never sees it.
    private var connectedPeripheralPoll: Timer?

    private let protocolHandler = ATVVProtocol()
    private var streamID: UInt8 = 0
    private var isStreaming = false
    private var levelSumSq: Int64 = 0
    private var levelCount: Int = 0
    private var levelPeak: Int = 0
    private var levelLogAt = Date.distantPast
    private var diagFrameCount = 0
    private var streamFrameCount = 0
    private var streamStartedAt = Date.distantPast
    private var streamSampleCount = 0
    private var streamPeak = 0
    private var streamSumSquares: Int64 = 0
    private var streamClippedSampleCount = 0
    private var pendingAudioSyncCount = 0
    private var streamAudioSyncCount = 0
    private var wavRecorder: WavRecorder?
    // Verbose PCM diagnostics are off unless explicitly requested.
    private static let diagnosticsEnabled =
        ProcessInfo.processInfo.environment["MIA_DIAGNOSTICS"] == "1"
    private var lastStartSearchAt = Date.distantPast
    private var lastMicOpenAt = Date.distantPast
    private var closeTimeoutWorkItem: DispatchWorkItem?
    private var hasResetSessionForConnection = false

    init(
        nameHint: String?,
        savedUUIDFilename: String,
        recordingPrefix: String,
        logTag: String,
        resetSessionOnConnect: Bool
    ) {
        self.nameHint = nameHint
        self.savedUUIDPath = URL(
            fileURLWithPath: Self.appSupportDirectory
        ).appendingPathComponent(savedUUIDFilename).path
        self.recordingPrefix = recordingPrefix
        self.logTag = logTag
        self.resetSessionOnConnect = resetSessionOnConnect
        super.init()
        central = CBCentralManager(delegate: self, queue: .main)
    }

    func start() {
        isStarted = true
        if central.state == .poweredOn {
            tryDirectConnect()
        } else {
            print("[\(logTag)] 等待蓝牙就绪…")
        }
    }

    func stop() {
        isStarted = false
        stopConnectedPeripheralPoll()
        closeTimeoutWorkItem?.cancel()
        closeTimeoutWorkItem = nil
        stopKeepAliveTimer()
        if isStreaming {
            if let close = try? protocolHandler.micCloseCommand(streamID: streamID) {
                writeCommand(close)
                print("[\(logTag)] TX micClose before shutdown")
            }
            finishAudioStreamLocally(reason: 0xFD)
        }
        if let peripheral {
            central.cancelPeripheralConnection(peripheral)
        }
        self.peripheral = nil
        audioChar = nil
        commandChar = nil
        controlChar = nil
    }

    func setRecordingEnabled(_ enabled: Bool) {
        AppStorage.recordingEnabled = enabled
        if enabled, isStreaming, wavRecorder == nil {
            wavRecorder = WavRecorder.createNext(prefix: recordingPrefix)
        } else if !enabled, let recorder = wavRecorder {
            recorder.close()
            wavRecorder = nil
        }
    }

    @discardableResult
    func openMicrophone(
        bypassDebounce: Bool = false
    ) -> RemoteMicrophoneOpenResult {
        guard !isStreaming else { return .alreadyStreaming }
        guard peripheral != nil, commandChar != nil else {
            print("[\(logTag)] micOpen waiting for writable characteristic")
            return .retryAfter(0.15)
        }
        let now = Date()
        let elapsed = now.timeIntervalSince(lastMicOpenAt)
        guard bypassDebounce || elapsed >= 0.35 else {
            print("[\(logTag)] micOpen duplicate suppressed")
            return .retryAfter(max(0.01, 0.35 - elapsed))
        }
        do {
            protocolHandler.prepareForAudioStream()
            pendingAudioSyncCount = 0
            guard writeCommand(try protocolHandler.micOpenCommand()) else {
                return .retryAfter(0.15)
            }
            lastMicOpenAt = now
            print("[\(logTag)] TX micOpen requested by gesture")
            return .sent
        } catch {
            print(
                "[\(logTag)] micOpen gesture error: " +
                error.localizedDescription
            )
            return .failed(error.localizedDescription)
        }
    }

    func closeMicrophone(force: Bool = false) {
        // `isStreaming` is driven by the remote's AUDIO_START/AUDIO_STOP
        // notifications.  A lost stop packet must not make the close action
        // a no-op: the remote can still be sending frames and keep-alives.
        guard force || isStreaming else {
            print("[\(logTag)] micClose ignored; local stream already idle")
            return
        }
        if !isStreaming {
            // Keep the UI and AudioPipe honest even when the peripheral's
            // start/stop notifications were lost and only a forced reset is
            // available.
            onLevel?(-120, 0)
        }
        // Stop extending the session as soon as the user finishes a gesture.
        // X6 does not reliably stop ATVV by itself; leaving keep-alive active
        // saturates its BLE link and delays/drops ordinary HID keyboard data.
        stopKeepAliveTimer()
        do {
            writeCommand(
                try protocolHandler.micCloseCommand(streamID: streamID)
            )
            print("[\(logTag)] TX micClose requested by gesture")
            closeTimeoutWorkItem?.cancel()
            let timeout = DispatchWorkItem { [weak self] in
                guard let self, self.isStreaming else { return }
                print("[\(self.logTag)] AUDIO_STOP timeout; forcing local cleanup")
                self.finishAudioStreamLocally(reason: 0xFE)
            }
            closeTimeoutWorkItem = timeout
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.0, execute: timeout)
        } catch {
            print(
                "[\(logTag)] micClose gesture error: " +
                error.localizedDescription
            )
        }
    }

    func clearRecordings() {
        if let recorder = wavRecorder {
            recorder.close()
            wavRecorder = nil
        }
        AppStorage.clearFiles(in: AppStorage.recordingsDirectory)
    }

    /// If we have a saved UUID from a previous scan, ask CoreBluetooth for
    /// the peripheral directly. This works even when the device is HID-paired
    /// (advertisement scan returns nothing in that case).
    private func tryDirectConnect() {
        // Prefer the currently connected physical device over a persisted
        // UUID. This makes same-model replacement remotes work without
        // manually deleting x6-uuid.txt from the previous unit.
        let connected = central.retrieveConnectedPeripherals(
            withServices: [Self.serviceUUID]
        )
        if let candidate = connected.first(where: matches) {
            print(
                "[\(logTag)] ATVV 已连接设备直接 connect: " +
                "\(candidate.name ?? "(no name)") \(candidate.identifier)"
            )
            connect(to: candidate)
            return
        }

        if let uuidString = try? String(
            contentsOfFile: savedUUIDPath,
            encoding: .utf8
        ),
            let uuid = UUID(
                uuidString: uuidString.trimmingCharacters(
                    in: .whitespacesAndNewlines
                )
            )
        {
            let known = central.retrievePeripherals(withIdentifiers: [uuid])
            if let candidate = known.first, matches(candidate) {
                print("[\(logTag)] 已存 UUID 直接 connect: \(candidate.identifier)")
                connect(to: candidate, allowTimeoutFallback: true)
                return
            }
        }

        print("[\(logTag)] 无已存 UUID/ATVV 连接，转扫描")
        beginScan()
    }

    private func beginScan() {
        print("[\(logTag)] 扫描中（按名字 \(nameHint ?? "?") + ATVV service 过滤）")
        central.scanForPeripherals(
            withServices: nil,
            options: [CBCentralManagerScanOptionAllowDuplicatesKey: false]
        )
        startConnectedPeripheralPoll()
    }

    private func startConnectedPeripheralPoll() {
        stopConnectedPeripheralPoll()
        connectedPeripheralPoll = Timer.scheduledTimer(
            withTimeInterval: 3.0,
            repeats: true
        ) { [weak self] _ in
            guard let self, self.isStarted,
                  self.central.state == .poweredOn,
                  self.connectedPeripheralID == nil
            else { return }
            let connected = self.central.retrieveConnectedPeripherals(
                withServices: [Self.serviceUUID]
            )
            guard let candidate = connected.first(where: self.matches) else {
                return
            }
            print(
                "[\(self.logTag)] 扫描期间发现系统已连接设备，直接 connect: " +
                "\(candidate.identifier)"
            )
            self.connect(to: candidate)
        }
    }

    private func stopConnectedPeripheralPoll() {
        connectedPeripheralPoll?.invalidate()
        connectedPeripheralPoll = nil
    }

    fileprivate func saveUUID(_ id: UUID) {
        let path = savedUUIDPath
        let url = URL(fileURLWithPath: path)
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try? id.uuidString.write(to: url, atomically: true, encoding: .utf8)
        print("[\(logTag)] 已保存 UUID: \(id.uuidString)")
    }

    fileprivate func connect(
        to p: CBPeripheral,
        allowTimeoutFallback: Bool = false
    ) {
        guard matches(p) else { return }
        connectTimeout?.cancel()
        stopConnectedPeripheralPoll()
        peripheral = p
        connectedPeripheralID = nil
        p.delegate = self
        central.stopScan()
        central.connect(p, options: nil)
        saveUUID(p.identifier)
        print("[\(logTag)] 连接 \(p.name ?? "(no name)") (\(p.identifier))…")

        guard allowTimeoutFallback else { return }
        let id = p.identifier
        let work = DispatchWorkItem { [weak self, weak p] in
            guard let self, self.connectedPeripheralID != id else { return }
            print("[\(self.logTag)] 已存 UUID 连接超时，改为扫描新设备")
            if let p {
                self.central.cancelPeripheralConnection(p)
            }
            self.beginScan()
        }
        connectTimeout = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 4.0, execute: work)
    }

    private func matches(_ candidate: CBPeripheral) -> Bool {
        guard let nameHint else { return true }
        return candidate.name?.localizedCaseInsensitiveContains(nameHint)
            == true
    }

    @discardableResult
    private func writeCommand(_ data: Data) -> Bool {
        guard let p = peripheral, let c = commandChar else { return false }
        if c.properties.contains(.write) {
            p.writeValue(data, for: c, type: .withResponse)
            return true
        } else if c.properties.contains(.writeWithoutResponse) {
            p.writeValue(data, for: c, type: .withoutResponse)
            return true
        } else {
            print("[\(logTag)] command characteristic is not writable")
            return false
        }
    }

    /// Send an ATVV keep-alive. Should be called every ~4 seconds while
    /// streaming to keep the device's audio stream open during long
    /// voice-key holds.
    private func sendKeepAlive() {
        guard isStreaming, let s = protocolHandler.codec else { return }
        do {
            let cmd = try protocolHandler.keepAliveCommand(streamID: streamID)
            writeCommand(cmd)
            // Quiet log; user only needs levels to debug.
        } catch {
            print("[\(logTag)] keep-alive error: \(error.localizedDescription)")
        }
        _ = s
    }

    /// Start a periodic timer that keeps the ATVV voice stream alive while
    /// the remote thinks the user is holding the voice button.
    private var keepAliveTimer: Timer?
    private func startKeepAliveTimer() {
        stopKeepAliveTimer()
        keepAliveTimer = Timer.scheduledTimer(
            withTimeInterval: 4.0,
            repeats: true
        ) { [weak self] _ in
            self?.sendKeepAlive()
        }
    }
    private func stopKeepAliveTimer() {
        keepAliveTimer?.invalidate()
        keepAliveTimer = nil
    }

    private func notifyStreaming(_ on: Bool) {
        let sr = (protocolHandler.codec == .adpcm16k) ? 16_000 : 8_000
        onStreamingChanged?(on, sr)
    }

    /// Completes the local side of an ATVV stream even when the peripheral
    /// drops the matching AUDIO_STOP notification.  This is deliberately
    /// idempotent so an eventual late AUDIO_STOP is harmless.
    private func finishAudioStreamLocally(reason: UInt8) {
        closeTimeoutWorkItem?.cancel()
        closeTimeoutWorkItem = nil
        let wasStreaming = isStreaming
        isStreaming = false
        protocolHandler.endAudioStream()
        stopKeepAliveTimer()
        if wasStreaming {
            notifyStreaming(false)
            onAudioStopped?(reason)
        }
        levelSumSq = 0
        levelCount = 0
        levelPeak = 0
        onLevel?(-120, 0)
        diagFrameCount = 0
        if let w = wavRecorder {
            w.close()
            print("[WAV] closed \(w.filename)")
            wavRecorder = nil
        }
        let streamRMS = streamSampleCount > 0
            ? sqrt(Double(streamSumSquares) / Double(streamSampleCount))
            : 0
        let streamDB = streamRMS > 0
            ? 20.0 * log10(streamRMS / 32768.0)
            : -120
        print(
            "[\(logTag)] AUDIO_SUMMARY streamID=\(streamID) " +
            "frames=\(streamFrameCount) samples=\(streamSampleCount) " +
            "peak=\(streamPeak) rms=\(Int(streamRMS.rounded())) " +
            "db=\(Int(streamDB.rounded())) " +
            "clipped=\(streamClippedSampleCount) syncs=\(streamAudioSyncCount)"
        )
        print("[\(logTag)] AUDIO_STOP reason=0x\(String(reason, radix: 16))")
    }
}

// MARK: - CBCentralManagerDelegate

extension BLEBridge: CBCentralManagerDelegate {
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        guard isStarted else { return }
        switch central.state {
        case .poweredOn:
            print("[\(logTag)] 蓝牙已就绪")
            tryDirectConnect()
        case .poweredOff:
            print("[\(logTag)] 蓝牙未开启")
        default:
            break
        }
    }

    func centralManager(_ central: CBCentralManager,
                        didDiscover peripheral: CBPeripheral,
                        advertisementData: [String: Any],
                        rssi RSSI: NSNumber) {
        let n = peripheral.name ?? "(no name)"
        let services = (advertisementData[CBAdvertisementDataServiceUUIDsKey] as? [CBUUID] ?? [])
        let hasATVV = services.contains(Self.serviceUUID)
        let short = services.map { String($0.uuidString.prefix(8)) }
        // Match the configured device name as well as the shared ATVV service;
        // multiple voice remotes can advertise the same service UUID.
        if matches(peripheral) {
            print("[\(logTag)] 候选: \(n) (\(peripheral.identifier)), services=\(short), rssi=\(RSSI)")
            connect(to: peripheral)
        }
        _ = hasATVV
    }

    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        connectTimeout?.cancel()
        connectTimeout = nil
        connectedPeripheralID = peripheral.identifier
        hasResetSessionForConnection = false
        print("[\(logTag)] 已连接")
        onConnectionChanged?(true)
        peripheral.discoverServices([Self.serviceUUID])
    }

    func centralManager(_ central: CBCentralManager,
                        didFailToConnect peripheral: CBPeripheral,
                        error: Error?) {
        connectTimeout?.cancel()
        connectTimeout = nil
        connectedPeripheralID = nil
        print("[\(logTag)] 连接失败: \(error?.localizedDescription ?? "unknown")")
        onConnectionChanged?(false)
        beginScan()
    }

    func centralManager(_ central: CBCentralManager,
                        didDisconnectPeripheral peripheral: CBPeripheral,
                        error: Error?) {
        // A pending attempt cancelled by the connect timeout also lands here;
        // its timeout handler already started scanning.
        let wasConnected = connectedPeripheralID == peripheral.identifier
        connectTimeout?.cancel()
        connectTimeout = nil
        closeTimeoutWorkItem?.cancel()
        closeTimeoutWorkItem = nil
        connectedPeripheralID = nil
        print("[\(logTag)] 断开: \(error?.localizedDescription ?? "ok")")
        onConnectionChanged?(false)
        finishAudioStreamLocally(reason: 0xFC)
        // Reconnect the way launch does: the remote usually comes back as a
        // system-connected HID device that no longer advertises.
        guard wasConnected else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) { [weak self] in
            guard let self, self.isStarted else { return }
            self.tryDirectConnect()
        }
    }
}

// MARK: - CBPeripheralDelegate

extension BLEBridge: CBPeripheralDelegate {
    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        guard error == nil, let services = peripheral.services else {
            print("[\(logTag)] 服务发现失败: \(error?.localizedDescription ?? "?")")
            return
        }
        print("[\(logTag)] 发现 \(services.count) 个 service")
        for s in services {
            let tag = s.uuid == Self.serviceUUID ? " ←ATVV" : ""
            print("[\(logTag)]   svc \(s.uuid.uuidString.prefix(8))…\(tag)")
            // Discover ALL characteristics on every service so we can also
            // see HOGP (HID-over-GATT) button channels if they exist.
            peripheral.discoverCharacteristics(nil, for: s)
        }
    }

    func peripheral(_ peripheral: CBPeripheral,
                    didDiscoverCharacteristicsFor service: CBService,
                    error: Error?) {
        guard error == nil, let chars = service.characteristics else { return }
        for c in chars {
            let props = describeProperties(c.properties)
            let isATVV = service.uuid == Self.serviceUUID
            print("[\(logTag)]   char \(c.uuid.uuidString.prefix(8)) [\(props)]")
            switch c.uuid {
            case Self.audioUUID:
                audioChar = c
                peripheral.setNotifyValue(true, for: c)
                print("[\(logTag)]   → audio subscribed; isNotifying=\(c.isNotifying)")
                if isATVV { atvvCharsComplete = true }
            case Self.commandUUID:
                commandChar = c
                print("[\(logTag)]   → command stored")
                if c.properties.contains(.notify)
                    || c.properties.contains(.indicate)
                {
                    peripheral.setNotifyValue(true, for: c)
                    print(
                        "[\(logTag)]   → command response subscribed; " +
                        "isNotifying=\(c.isNotifying)"
                    )
                }
            case Self.controlUUID:
                controlChar = c
                peripheral.setNotifyValue(true, for: c)
                print("[\(logTag)]   → control subscribed; isNotifying=\(c.isNotifying)")
                if isATVV { atvvCharsComplete = true }
            default:
                // Auto-subscribe to any notify-capable characteristic on this
                // service — we may find button events on HOGP here.
                if c.properties.contains(.notify) || c.properties.contains(.indicate) {
                    peripheral.setNotifyValue(true, for: c)
                    print("[\(logTag)]   → auto-subscribed (notify); isNotifying=\(c.isNotifying)")
                }
            }
        }
        if atvvCharsComplete {
            // All three ATVV chars found: kick off capabilities handshake.
            writeCommand(protocolHandler.getCapabilitiesCommand)
            print("[\(logTag)] TX getCapabilities")
            atvvCharsComplete = false
        }
    }

    private func describeProperties(_ p: CBCharacteristicProperties) -> String {
        var parts: [String] = []
        if p.contains(.read) { parts.append("read") }
        if p.contains(.write) { parts.append("write") }
        if p.contains(.writeWithoutResponse) { parts.append("writeNoResp") }
        if p.contains(.notify) { parts.append("notify") }
        if p.contains(.indicate) { parts.append("indicate") }
        return parts.joined(separator: ",")
    }

    func peripheral(_ peripheral: CBPeripheral,
                    didUpdateValueFor characteristic: CBCharacteristic,
                    error: Error?) {
        if let error {
            print("[\(logTag)] char update error: \(error.localizedDescription)")
            return
        }
        guard let data = characteristic.value else { return }
        let len = data.count
        switch characteristic.uuid {
        case Self.controlUUID:
            handleControl(data)
        case Self.commandUUID:
            // Some ATVV remotes, including X6, return capabilities on the
            // bidirectional command characteristic rather than control.
            handleControl(data)
        case Self.audioUUID:
            handleAudio(data)
        default:
            print("[\(logTag)] unexpected char update \(characteristic.uuid.uuidString.prefix(8)), \(len)B")
        }
    }

    private func handleControl(_ data: Data) {
        let ev = protocolHandler.parseControl(data)
        switch ev {
        case .startSearch:
            // If already streaming, or if START_SEARCH repeats immediately,
            // keep the current session instead of opening it twice.
            let now = Date()
            if isStreaming { return }
            if now.timeIntervalSince(lastStartSearchAt) < 0.5 { return }
            lastStartSearchAt = now
            openMicrophone()
            print("[\(logTag)] TX micOpen (debounced)")
        case .capabilities(let caps):
            do {
                try protocolHandler.acceptCapabilities(caps)
                print(
                    "[\(logTag)] caps ok v\(caps.version) " +
                    "codec=\(protocolHandler.codec!) " +
                    "interaction=0x\(String(format: "%02x", caps.interactionModel)) " +
                    "frame=\(caps.frameSize)"
                )
                resetStaleSessionIfNeeded()
            } catch {
                print("[\(logTag)] caps error: \(error.localizedDescription)")
            }
        case .audioStart(let reason, let codec, let sid):
            closeTimeoutWorkItem?.cancel()
            closeTimeoutWorkItem = nil
            protocolHandler.beginAudioStream(codec: codec)
            streamID = sid
            isStreaming = true
            notifyStreaming(true)
            onAudioStarted?(reason, codec.sampleRate)
            diagFrameCount = 0
            streamFrameCount = 0
            streamStartedAt = Date()
            streamSampleCount = 0
            streamPeak = 0
            streamSumSquares = 0
            streamClippedSampleCount = 0
            streamAudioSyncCount = pendingAudioSyncCount
            pendingAudioSyncCount = 0
            wavRecorder = AppStorage.recordingEnabled
                ? WavRecorder.createNext(prefix: recordingPrefix)
                : nil
            startKeepAliveTimer()
            print(
                "[\(logTag)] AUDIO_START reason=" +
                String(format: "0x%02x", reason) +
                " streamID=\(sid)"
            )
        case .audioStop(let reason):
            finishAudioStreamLocally(reason: reason)
            /*
            isStreaming = false
            protocolHandler.endAudioStream()
            notifyStreaming(false)
            levelSumSq = 0
            levelCount = 0
            levelPeak = 0
            onLevel?(-120, 0)
            diagFrameCount = 0
            stopKeepAliveTimer()
            if let w = wavRecorder {
                w.close()
                print("[WAV] closed \(w.filename)")
                wavRecorder = nil
            }
            let streamRMS = streamSampleCount > 0
                ? sqrt(Double(streamSumSquares) / Double(streamSampleCount))
                : 0
            let streamDB = streamRMS > 0
                ? 20.0 * log10(streamRMS / 32768.0)
                : -120
            print(
                "[\(logTag)] AUDIO_SUMMARY streamID=\(streamID) " +
                "frames=\(streamFrameCount) samples=\(streamSampleCount) " +
                "peak=\(streamPeak) rms=\(Int(streamRMS.rounded())) " +
                "db=\(String(format: "%.1f", streamDB)) " +
                "clipped=\(streamClippedSampleCount) syncs=\(streamAudioSyncCount)"
            )
            print("[\(logTag)] AUDIO_STOP reason=0x\(String(format: "%02x", reason))")
            */
        case .audioSync(let codec, let sequence, let pred, let stepIndex):
            protocolHandler.applyAudioSync(
                codec: codec,
                sequence: sequence,
                predictor: pred,
                stepIndex: stepIndex
            )
            if isStreaming {
                streamAudioSyncCount += 1
            } else {
                pendingAudioSyncCount += 1
            }
            print(
                "[\(logTag)] AUDIO_SYNC codec=\(codec) sequence=\(sequence) " +
                "predictor=\(pred) step=\(stepIndex) " +
                "phase=\(isStreaming ? "streaming" : "opening")"
            )
        case .micOpenError(let code):
            print("[\(logTag)] MIC_OPEN 错误: 0x\(String(format: "%04x", code))")
            onMicrophoneOpenFailed?(code)
        case .unknown(let d):
            // Capabilities-handshake uses 0x0B on the control char sometimes;
            // absorb instead of failing noisily.
            if data.first == 0x0B {
                if let caps = ATVVProtocol.parseCapabilities(data) {
                    do {
                        try protocolHandler.acceptCapabilities(caps)
                        print(
                            "[\(logTag)] caps (fallback) v\(caps.version) " +
                            "codec=\(protocolHandler.codec!) " +
                            "interaction=0x\(String(format: "%02x", caps.interactionModel)) " +
                            "frame=\(caps.frameSize)"
                        )
                        resetStaleSessionIfNeeded()
                    } catch {
                        print("[\(logTag)] caps fallback error: \(error.localizedDescription)")
                    }
                }
            }
            // Silently ignore other unknown events.
            _ = d
        }
    }

    private func resetStaleSessionIfNeeded() {
        guard resetSessionOnConnect, !hasResetSessionForConnection else {
            return
        }
        hasResetSessionForConnection = true
        if let close = try? protocolHandler.micCloseCommand(streamID: 0) {
            writeCommand(close)
            print("[\(logTag)] TX micClose reset")
        }
    }

    private func handleAudio(_ data: Data) {
        guard isStreaming, let frame = protocolHandler.decodeAudio(data) else { return }
        if frame.samples.isEmpty { return }
        let framePeak = frame.samples.reduce(into: 0) {
            $0 = max($0, abs(Int($1)))
        }
        let frameSumSquares = frame.samples.reduce(into: Int64(0)) {
            $0 += Int64($1) * Int64($1)
        }
        streamFrameCount += 1
        if streamFrameCount == 1 {
            onFirstAudioFrame?()
        }
        streamSampleCount += frame.samples.count
        streamPeak = max(streamPeak, framePeak)
        streamSumSquares += frameSumSquares
        streamClippedSampleCount += frame.samples.reduce(into: 0) {
            if abs(Int($1)) >= 32_760 { $0 += 1 }
        }
        if streamFrameCount == 1 {
            let latency = Date().timeIntervalSince(streamStartedAt) * 1000
            print(
                "[\(logTag)] AUDIO_FRAMES count=1 " +
                "samples=\(streamSampleCount) recentPeak=\(framePeak) " +
                "firstFrameMs=\(Int(latency.rounded()))"
            )
        } else if streamFrameCount.isMultiple(of: 200) {
            print(
                "[\(logTag)] AUDIO_FRAMES count=\(streamFrameCount) " +
                "samples=\(streamSampleCount) recentPeak=\(framePeak)"
            )
        }

        // Diagnostic: dump first few raw bytes & first few decoded samples
        if Self.diagnosticsEnabled, diagFrameCount < 5 {
            let rawHex = data.prefix(16).map { String(format: "%02X", $0) }.joined(separator: " ")
            let samHex = frame.samples.prefix(8).map { String($0) }.joined(separator: ",")
            print("[AUDIO-DIAG] frame#\(diagFrameCount) raw[\(rawHex)] decoded[\(samHex)]")
            diagFrameCount += 1
        }

        // Optional developer-only recording. Production builds never create
        // a WAV because `recordingEnabled` is false.
        if let w = wavRecorder {
            w.appendSamples(frame.samples)
            w.appendRawBytes(data)
        }

        AudioPipe.shared.feed(samples: frame.samples)
        let needsLevel = onLevel != nil || Self.diagnosticsEnabled
        if needsLevel {
            levelSumSq += frame.samples.reduce(into: 0) {
                $0 += Int64($1) * Int64($1)
            }
            levelCount += frame.samples.count
            levelPeak = max(
                levelPeak,
                frame.samples.map { Int(abs(Int($0))) }.max() ?? 0
            )
        }
        if needsLevel, Date().timeIntervalSince(levelLogAt) >= 0.1 {
            let rms = levelCount > 0 ? sqrt(Double(levelSumSq) / Double(levelCount)) : 0
            let peak = levelPeak
            let db = rms > 0 ? 20.0 * log10(rms / 32768.0) : -120
            let bar = meterBar(level: db)
            if Self.diagnosticsEnabled {
                print(String(format: "[LVL] %@ ATVV rms=%.0f peak=%d  %.0f dBFS",
                             bar, rms, peak, db))
            }
            onLevel?(db, peak)
            levelSumSq = 0
            levelCount = 0
            levelPeak = 0
            levelLogAt = Date()
        }
    }

    /// Convert dBFS to a tiny ASCII bar for at-a-glance logs.
    private func meterBar(level: Double) -> String {
        let clamped = max(-60, min(0, level))
        let n = (clamped + 60) / 6   // 0..10 segments
        return "[" + String(repeating: "▓", count: Int(n)) +
                     String(repeating: "░", count: 10 - Int(n)) + "]"
    }
}
