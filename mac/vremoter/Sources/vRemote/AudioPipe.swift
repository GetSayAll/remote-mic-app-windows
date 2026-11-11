// vRemote dual-microphone audio pipe.
//
// MacBook microphone -> AVCaptureSession ----┐
//                                             ├-> aligned equal mix -> vRemoteDr 2ch
// X6 ATVV ADPCM -> decoded Int16 PCM --------┘
//
// Both microphones are peers. Neither source permanently wins or ducks the
// other. The Mac path is delayed slightly to compensate for BLE transport
// latency, then the two available signals are averaged and hard-limited.

import AVFoundation
import AudioToolbox
import CoreAudio
import CoreMedia
import Foundation

final class AudioPipe: NSObject, AVCaptureAudioDataOutputSampleBufferDelegate {
    static let shared = AudioPipe()

    private enum InputSource {
        case mac
        case remote
    }

    private static let targetDeviceName = "vRemoteDr 2ch"
    private static let outputSampleRate: Double = 48_000
    private static let outputChannels: AVAudioChannelCount = 2

    // X6's decoded ATVV samples are much quieter than CoreAudio microphone
    // samples. This +20 dB calibration is inherited from the proven V1 path;
    // it is input calibration, not source priority.
    private static let remoteGain: Float = 10.0

    // The Mac capture path usually arrives before BLE voice frames. Keeping a
    // small Mac backlog makes the same spoken syllable from both microphones
    // meet in the same render window. This first experimental value is shown
    // in logs and can be tuned from real recordings later.
    private static let macAlignmentDelaySeconds = 0.12
    private static let macAlignmentDelayFrames = Int(
        outputSampleRate * macAlignmentDelaySeconds
    )

    private let captureSession = AVCaptureSession()
    private let captureQueue = DispatchQueue(
        label: "local.simaqingfeng.vRemote.capture",
        qos: .userInteractive
    )

    private let stateLock = NSLock()
    private var mixActive = false
    private var macInputEnabled = AppStorage.macInputEnabled
    private var remoteInputEnabled = AppStorage.remoteInputEnabled
    private var outputDeviceID: AudioDeviceID?
    private var outputIOProcID: AudioDeviceIOProcID?

    private var pendingMac: [Float] = []
    private var pendingMacIndex = 0
    private var pendingRemote: [Float] = []
    private var pendingRemoteIndex = 0

    private var scheduledMacBuffers = 0
    private var scheduledRemoteBuffers = 0
    private var scheduledPeak: Float = 0
    private var renderCallbackCount = 0
    private var remoteSessionScheduledBuffers = 0
    private var remoteSessionRenderedFrames = 0
    private var loggedFirstRemoteRender = false
    private var diagnosticsTimer: Timer?

    var onMacLevel: ((Double) -> Void)?
    var onRouteChanged: ((Bool) -> Void)?
    private var macLevelAt = Date.distantPast
    private var captureConfigured = false

    private override init() {
        super.init()

        if let deviceID = Self.findOutputDevice(named: Self.targetDeviceName) {
            startOutputDevice(deviceID)
        } else {
            print(
                "[AUDIO] ⚠️ 找不到 \(Self.targetDeviceName)，" +
                "请确认 vRemoteDriver.driver 已安装"
            )
        }

        if ProcessInfo.processInfo.environment["MIA_DIAGNOSTICS"] == "1" {
            startDiagnosticsTimer()
        }
        if macInputEnabled {
            requestBuiltInMicAccess()
        } else {
            print("[AUDIO] MacBook 麦克风未勾选，不启动采集")
        }
    }

    // MARK: - Public input API

    func feed(samples: [Int16], inputSampleRate: Double = 16_000) {
        guard !samples.isEmpty else { return }
        let state = inputState
        guard state.mixActive, state.remoteEnabled else { return }
        let floatSamples = samples.map { sample -> Float in
            let amplified = Float(sample) / 32768.0 * Self.remoteGain
            return max(-1.0, min(1.0, amplified))
        }
        schedule(
            monoSamples: floatSamples,
            sampleRate: inputSampleRate,
            source: .remote
        )
    }

    /// Kept under the V1 method name so the proven X6 session state machine
    /// remains unchanged. In vRemote, `active` means that the dual-input mix
    /// is open; it no longer selects one microphone over another.
    func setRemoteActive(_ active: Bool) {
        stateLock.lock()
        let changed = mixActive != active
        mixActive = active
        clearPendingBuffersLocked()
        if active, changed {
            remoteSessionScheduledBuffers = 0
            remoteSessionRenderedFrames = 0
            loggedFirstRemoteRender = false
        }
        stateLock.unlock()

        guard changed else { return }
        print(
            active
                ? "[AUDIO] 双麦克风混音开启 · X6 + MacBook · Mac 对齐延迟 120ms"
                : "[AUDIO] 双麦克风混音关闭"
        )
        onRouteChanged?(active)
    }

    func setInputEnabled(mac: Bool, remote: Bool) {
        stateLock.lock()
        let macChanged = macInputEnabled != mac
        let remoteChanged = remoteInputEnabled != remote
        macInputEnabled = mac
        remoteInputEnabled = remote
        if !mac {
            pendingMac.removeAll(keepingCapacity: true)
            pendingMacIndex = 0
        }
        if !remote {
            pendingRemote.removeAll(keepingCapacity: true)
            pendingRemoteIndex = 0
        }
        stateLock.unlock()

        AppStorage.macInputEnabled = mac
        AppStorage.remoteInputEnabled = remote

        if macChanged {
            setMacCaptureEnabled(mac)
            if !mac { onMacLevel?(-120) }
        }
        if macChanged || remoteChanged {
            print(
                "[AUDIO] 输入选择更新 · MacBook=\(mac ? "ON" : "OFF") " +
                "X6=\(remote ? "ON" : "OFF")"
            )
            onRouteChanged?(mixActive)
        }
    }

    var isMacInputEnabled: Bool { inputState.macEnabled }
    var isRemoteInputEnabled: Bool { inputState.remoteEnabled }
    var isOutputDeviceAvailable: Bool { outputDeviceID != nil }

    func stop() {
        diagnosticsTimer?.invalidate()
        diagnosticsTimer = nil

        if captureSession.isRunning {
            captureSession.stopRunning()
        }

        if let deviceID = outputDeviceID, let ioProcID = outputIOProcID {
            let stopStatus = AudioDeviceStop(deviceID, ioProcID)
            let destroyStatus = AudioDeviceDestroyIOProcID(deviceID, ioProcID)
            print(
                "[AUDIO] vRemote IOProc 已释放: " +
                "stop=\(stopStatus), destroy=\(destroyStatus)"
            )
        }
        outputIOProcID = nil
        outputDeviceID = nil
    }

    // MARK: - Built-in microphone capture

    private func requestBuiltInMicAccess() {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            startBuiltInMicCapture()
        case .notDetermined:
            print("[AUDIO] 请求 MacBook 麦克风权限…")
            AVCaptureDevice.requestAccess(for: .audio) { [weak self] granted in
                DispatchQueue.main.async {
                    if granted {
                        self?.startBuiltInMicCapture()
                    } else {
                        print("[AUDIO] ⚠️ MacBook 麦克风权限被拒绝")
                    }
                }
            }
        default:
            print("[AUDIO] ⚠️ 没有 MacBook 麦克风权限；当前只能收到 X6 音频")
        }
    }

    private func startBuiltInMicCapture() {
        if captureConfigured {
            captureQueue.async { [weak self] in
                guard let self, !self.captureSession.isRunning else { return }
                self.captureSession.startRunning()
                print("[AUDIO] MacBook 麦克风采集已恢复")
            }
            return
        }
        let discovery = AVCaptureDevice.DiscoverySession(
            deviceTypes: [.builtInMicrophone],
            mediaType: .audio,
            position: .unspecified
        )
        let device = discovery.devices.first {
            $0.uniqueID == "BuiltInMicrophoneDevice"
        } ?? discovery.devices.first {
            $0.localizedName.localizedCaseInsensitiveContains("MacBook")
        }
        guard let device else {
            print("[AUDIO] ⚠️ 找不到 MacBook 内置麦克风")
            return
        }

        do {
            let input = try AVCaptureDeviceInput(device: device)
            let output = AVCaptureAudioDataOutput()
            output.audioSettings = [
                AVFormatIDKey: kAudioFormatLinearPCM,
                AVLinearPCMBitDepthKey: 32,
                AVLinearPCMIsFloatKey: true,
                AVLinearPCMIsNonInterleaved: false,
                AVNumberOfChannelsKey: 1,
            ]
            output.setSampleBufferDelegate(self, queue: captureQueue)

            captureSession.beginConfiguration()
            guard captureSession.canAddInput(input),
                  captureSession.canAddOutput(output)
            else {
                captureSession.commitConfiguration()
                print("[AUDIO] ⚠️ 无法建立 MacBook 麦克风采集会话")
                return
            }
            captureSession.addInput(input)
            captureSession.addOutput(output)
            captureSession.commitConfiguration()
            captureConfigured = true

            captureQueue.async { [weak self] in
                guard let self else { return }
                self.captureSession.startRunning()
                print(
                    "[AUDIO] MacBook 麦克风采集已启动: " +
                    "\(device.localizedName), running=\(self.captureSession.isRunning)"
                )
            }
        } catch {
            print("[AUDIO] ⚠️ MacBook 麦克风采集失败: \(error.localizedDescription)")
        }
    }

    func captureOutput(
        _ output: AVCaptureOutput,
        didOutput sampleBuffer: CMSampleBuffer,
        from connection: AVCaptureConnection
    ) {
        guard isMacInputEnabled,
              let formatDescription = CMSampleBufferGetFormatDescription(sampleBuffer),
              let streamDescription = CMAudioFormatDescriptionGetStreamBasicDescription(
                formatDescription
              ),
              streamDescription.pointee.mFormatID == kAudioFormatLinearPCM,
              streamDescription.pointee.mBitsPerChannel == 32,
              streamDescription.pointee.mFormatFlags & kAudioFormatFlagIsFloat != 0,
              let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer)
        else { return }

        let byteCount = CMBlockBufferGetDataLength(blockBuffer)
        guard byteCount >= MemoryLayout<Float>.size else { return }
        var data = Data(count: byteCount)
        let copyStatus = data.withUnsafeMutableBytes { bytes in
            CMBlockBufferCopyDataBytes(
                blockBuffer,
                atOffset: 0,
                dataLength: byteCount,
                destination: bytes.baseAddress!
            )
        }
        guard copyStatus == kCMBlockBufferNoErr else { return }

        let channels = max(1, Int(streamDescription.pointee.mChannelsPerFrame))
        let floats: [Float] = data.withUnsafeBytes { rawBuffer in
            let values = rawBuffer.bindMemory(to: Float.self)
            if channels == 1 {
                return Array(values)
            }
            let frameCount = values.count / channels
            return (0..<frameCount).map { frame in
                var sum: Float = 0
                for channel in 0..<channels {
                    sum += values[frame * channels + channel]
                }
                return sum / Float(channels)
            }
        }

        if Date().timeIntervalSince(macLevelAt) >= 0.1 {
            let sumSquares = floats.reduce(0.0) { $0 + Double($1 * $1) }
            let rms = floats.isEmpty ? 0 : sqrt(sumSquares / Double(floats.count))
            let db = rms > 0 ? 20.0 * log10(rms) : -120
            onMacLevel?(db)
            macLevelAt = Date()
        }

        guard isMixActive else { return }
        schedule(
            monoSamples: floats,
            sampleRate: streamDescription.pointee.mSampleRate,
            source: .mac
        )
    }

    // MARK: - Queues and resampling

    private var isMixActive: Bool {
        stateLock.lock()
        defer { stateLock.unlock() }
        return mixActive
    }

    private var inputState: (mixActive: Bool, macEnabled: Bool, remoteEnabled: Bool) {
        stateLock.lock()
        defer { stateLock.unlock() }
        return (mixActive, macInputEnabled, remoteInputEnabled)
    }

    private func setMacCaptureEnabled(_ enabled: Bool) {
        if enabled {
            requestBuiltInMicAccess()
            return
        }
        captureQueue.async { [weak self] in
            guard let self, self.captureSession.isRunning else { return }
            self.captureSession.stopRunning()
            print("[AUDIO] MacBook 麦克风采集已暂停")
        }
    }

    private func schedule(
        monoSamples: [Float],
        sampleRate: Double,
        source: InputSource
    ) {
        guard !monoSamples.isEmpty, sampleRate > 0 else { return }
        let resampled = Self.resample(
            monoSamples,
            from: sampleRate,
            to: Self.outputSampleRate
        )
        guard !resampled.isEmpty else { return }

        stateLock.lock()
        let sourceEnabled = source == .mac ? macInputEnabled : remoteInputEnabled
        guard mixActive, sourceEnabled else {
            stateLock.unlock()
            return
        }

        var firstRemoteBufferDescription: String?
        switch source {
        case .mac:
            compactMacBufferIfNeededLocked()
            pendingMac.append(contentsOf: resampled)
            scheduledMacBuffers += 1

        case .remote:
            compactRemoteBufferIfNeededLocked()
            pendingRemote.append(contentsOf: resampled)
            scheduledRemoteBuffers += 1
            remoteSessionScheduledBuffers += 1
            if remoteSessionScheduledBuffers == 1 {
                let peak = resampled.reduce(0) { max($0, abs($1)) }
                firstRemoteBufferDescription = String(
                    format: "[AUDIO] X6 首缓冲 frames=%d peak=%.4f",
                    resampled.count,
                    peak
                )
            }
        }
        scheduledPeak = max(
            scheduledPeak,
            resampled.reduce(0) { max($0, abs($1)) }
        )
        stateLock.unlock()

        if let firstRemoteBufferDescription {
            print(firstRemoteBufferDescription)
        }
    }

    private static func resample(
        _ input: [Float],
        from inputRate: Double,
        to outputRate: Double
    ) -> [Float] {
        if abs(inputRate - outputRate) < 1 {
            return input
        }
        let ratio = outputRate / inputRate
        let outputCount = max(1, Int((Double(input.count) * ratio).rounded(.down)))
        var output = [Float](repeating: 0, count: outputCount)
        for index in 0..<outputCount {
            let position = Double(index) / ratio
            let lower = min(Int(position), input.count - 1)
            let upper = min(lower + 1, input.count - 1)
            let fraction = Float(position - Double(lower))
            output[index] = input[lower] + (input[upper] - input[lower]) * fraction
        }
        return output
    }

    // MARK: - CoreAudio direct output

    private func startOutputDevice(_ deviceID: AudioDeviceID) {
        var ioProcID: AudioDeviceIOProcID?
        let createStatus = AudioDeviceCreateIOProcIDWithBlock(
            &ioProcID,
            deviceID,
            nil
        ) { [weak self] _, _, _, outputData, _ in
            self?.render(outputData)
        }
        guard createStatus == noErr, let ioProcID else {
            print("[AUDIO] ⚠️ 创建 vRemote IOProc 失败: OSStatus=\(createStatus)")
            return
        }

        let startStatus = AudioDeviceStart(deviceID, ioProcID)
        guard startStatus == noErr else {
            AudioDeviceDestroyIOProcID(deviceID, ioProcID)
            print("[AUDIO] ⚠️ 启动 vRemote IOProc 失败: OSStatus=\(startStatus)")
            return
        }

        outputDeviceID = deviceID
        outputIOProcID = ioProcID
        print("[AUDIO] vRemote IOProc 已启动: id=\(deviceID), 48000 Hz stereo")
    }

    private func render(_ outputData: UnsafeMutablePointer<AudioBufferList>) {
        let buffers = UnsafeMutableAudioBufferListPointer(outputData)
        guard let first = buffers.first else { return }
        let firstChannels = max(1, Int(first.mNumberChannels))
        let frameCount = Int(first.mDataByteSize) /
            MemoryLayout<Float>.size /
            firstChannels
        guard frameCount > 0 else { return }

        var mac = [Float](repeating: 0, count: frameCount)
        var remote = [Float](repeating: 0, count: frameCount)
        var mixed = [Float](repeating: 0, count: frameCount)
        var macTaken = 0
        var remoteTaken = 0
        var firstRemoteRenderDescription: String?

        stateLock.lock()
        if mixActive {
            let remoteAvailable = pendingRemote.count - pendingRemoteIndex
            if remoteAvailable > 0 {
                remoteTaken = min(frameCount, remoteAvailable)
                remote.replaceSubrange(
                    0..<remoteTaken,
                    with: pendingRemote[
                        pendingRemoteIndex..<(pendingRemoteIndex + remoteTaken)
                    ]
                )
                pendingRemoteIndex += remoteTaken
                remoteSessionRenderedFrames += remoteTaken
                if !loggedFirstRemoteRender {
                    loggedFirstRemoteRender = true
                    let peak = remote.reduce(0) { max($0, abs($1)) }
                    firstRemoteRenderDescription = String(
                        format: "[AUDIO] X6 首次输出 frames=%d peak=%.4f",
                        remoteTaken,
                        peak
                    )
                }
            }

            let macAvailable = pendingMac.count - pendingMacIndex
            let macReady = max(0, macAvailable - Self.macAlignmentDelayFrames)
            if macReady > 0 {
                macTaken = min(frameCount, macReady)
                mac.replaceSubrange(
                    0..<macTaken,
                    with: pendingMac[pendingMacIndex..<(pendingMacIndex + macTaken)]
                )
                pendingMacIndex += macTaken
            }

            compactRemoteBufferIfNeededLocked()
            compactMacBufferIfNeededLocked()
        }
        renderCallbackCount += 1
        stateLock.unlock()

        if let firstRemoteRenderDescription {
            print(firstRemoteRenderDescription)
        }

        for frame in 0..<frameCount {
            let hasMac = frame < macTaken
            let hasRemote = frame < remoteTaken
            let value: Float
            if hasMac && hasRemote {
                // Equal, source-agnostic mix. Averaging preserves headroom and
                // prevents the +6 dB overload caused by raw summation.
                value = (mac[frame] + remote[frame]) * 0.5
            } else if hasMac {
                value = mac[frame]
            } else if hasRemote {
                value = remote[frame]
            } else {
                value = 0
            }
            mixed[frame] = max(-1.0, min(1.0, value))
        }

        for buffer in buffers {
            guard let data = buffer.mData else { continue }
            let channelCount = max(1, Int(buffer.mNumberChannels))
            let writableFrames = min(
                frameCount,
                Int(buffer.mDataByteSize) /
                    MemoryLayout<Float>.size /
                    channelCount
            )
            let output = data.assumingMemoryBound(to: Float.self)
            for frame in 0..<writableFrames {
                for channel in 0..<channelCount {
                    output[frame * channelCount + channel] = mixed[frame]
                }
            }
        }
    }

    private func clearPendingBuffersLocked() {
        pendingMac.removeAll(keepingCapacity: true)
        pendingMacIndex = 0
        pendingRemote.removeAll(keepingCapacity: true)
        pendingRemoteIndex = 0
    }

    private func compactMacBufferIfNeededLocked() {
        if pendingMacIndex > 4_096 {
            pendingMac.removeFirst(pendingMacIndex)
            pendingMacIndex = 0
        }
        trimBufferIfNeededLocked(
            bufferCount: pendingMac.count,
            index: &pendingMacIndex,
            extraFrames: Self.macAlignmentDelayFrames
        )
    }

    private func compactRemoteBufferIfNeededLocked() {
        if pendingRemoteIndex > 4_096 {
            pendingRemote.removeFirst(pendingRemoteIndex)
            pendingRemoteIndex = 0
        }
        trimBufferIfNeededLocked(
            bufferCount: pendingRemote.count,
            index: &pendingRemoteIndex,
            extraFrames: 0
        )
    }

    private func trimBufferIfNeededLocked(
        bufferCount: Int,
        index: inout Int,
        extraFrames: Int
    ) {
        let maximumQueuedFrames = Int(Self.outputSampleRate * 2) + extraFrames
        let available = bufferCount - index
        if available > maximumQueuedFrames {
            index += available - maximumQueuedFrames
        }
    }

    private static func findOutputDevice(named targetName: String) -> AudioDeviceID? {
        var propertySize: UInt32 = 0
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        guard AudioObjectGetPropertyDataSize(
            AudioObjectID(kAudioObjectSystemObject),
            &address, 0, nil, &propertySize
        ) == noErr else { return nil }

        let count = Int(propertySize) / MemoryLayout<AudioDeviceID>.size
        var devices = [AudioDeviceID](repeating: 0, count: count)
        guard AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address, 0, nil, &propertySize, &devices
        ) == noErr else { return nil }

        for id in devices where id != 0 {
            var nameRef: Unmanaged<CFString>?
            var nameSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            var nameAddress = AudioObjectPropertyAddress(
                mSelector: kAudioObjectPropertyName,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            guard AudioObjectGetPropertyData(
                id, &nameAddress, 0, nil, &nameSize, &nameRef
            ) == noErr else { continue }
            guard let name = nameRef?.takeRetainedValue() as String? else { continue }
            if name.caseInsensitiveCompare(targetName) == .orderedSame {
                return id
            }
        }
        return nil
    }

    // MARK: - Diagnostics

    private func startDiagnosticsTimer() {
        diagnosticsTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) {
            [weak self] _ in
            guard let self else { return }
            self.stateLock.lock()
            let active = self.mixActive
            let macBuffers = self.scheduledMacBuffers
            let remoteBuffers = self.scheduledRemoteBuffers
            let peak = self.scheduledPeak
            let renders = self.renderCallbackCount
            let macQueued = self.pendingMac.count - self.pendingMacIndex
            let remoteQueued = self.pendingRemote.count - self.pendingRemoteIndex
            self.scheduledMacBuffers = 0
            self.scheduledRemoteBuffers = 0
            self.scheduledPeak = 0
            self.renderCallbackCount = 0
            self.stateLock.unlock()
            print(String(
                format:
                    "[AUDIO-METER] mix=%@ macBuffers=%d remoteBuffers=%d " +
                    "macQueued=%d remoteQueued=%d peak=%.4f renders=%d",
                active ? "on" : "off",
                macBuffers,
                remoteBuffers,
                macQueued,
                remoteQueued,
                peak,
                renders
            ))
        }
    }
}
