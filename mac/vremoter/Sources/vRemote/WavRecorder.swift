// Tiny WAV file writer for diagnostic audio capture.
// Saves 16-bit mono PCM at 16 kHz under Application Support only when the
// user explicitly enables diagnostic recording in the menu.

import Foundation

final class WavRecorder {
    static func createNext(prefix: String) -> WavRecorder? {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyyMMdd-HHmmss-SSS"
        let base = "\(prefix)-\(formatter.string(from: Date()))"
        let dir = AppStorage.recordingsDirectory
        AppStorage.ensureDirectory(dir)
        let wavURL = dir.appendingPathComponent("\(base).wav")
        let rawURL = dir.appendingPathComponent("\(base).raw.bin")
        return WavRecorder(wavURL: wavURL, rawURL: rawURL, basename: base)
    }

    let basename: String
    private let wavURL: URL
    private let rawURL: URL
    private var wavHandle: FileHandle?
    private var rawHandle: FileHandle?
    private var totalFrames: UInt64 = 0
    private let sampleRate: UInt32 = 16_000
    private let channels: UInt16 = 1
    private let bitsPerSample: UInt16 = 16

    init?(wavURL: URL, rawURL: URL, basename: String) {
        self.wavURL = wavURL
        self.rawURL = rawURL
        self.basename = basename
        // WAV
        try? FileManager.default.removeItem(at: wavURL)
        guard FileManager.default.createFile(atPath: wavURL.path, contents: nil),
              let wh = try? FileHandle(forWritingTo: wavURL) else { return nil }
        self.wavHandle = wh
        wh.write(Self.makeHeader(
            sampleRate: sampleRate, channels: channels,
            bitsPerSample: bitsPerSample, dataBytes: 0
        ))
        // Raw ATVV bytes
        try? FileManager.default.removeItem(at: rawURL)
        guard FileManager.default.createFile(atPath: rawURL.path, contents: nil),
              let rh = try? FileHandle(forWritingTo: rawURL) else {
            try? wh.close()
            return nil
        }
        self.rawHandle = rh
    }

    func appendSamples(_ samples: [Int16]) {
        guard let h = wavHandle, !samples.isEmpty else { return }
        var pcm = [UInt8](repeating: 0, count: samples.count * 2)
        for (i, s) in samples.enumerated() {
            let u = UInt16(bitPattern: Int16(s))
            pcm[i * 2]     = UInt8(u & 0xFF)
            pcm[i * 2 + 1] = UInt8((u >> 8) & 0xFF)
        }
        h.write(Data(pcm))
        totalFrames += UInt64(samples.count)
    }

    func appendRawBytes(_ data: Data) {
        guard let h = rawHandle else { return }
        h.write(data)
    }

    func close() {
        if let h = wavHandle {
            wavHandle = nil
            let dataBytes = UInt32(totalFrames) * UInt32(channels) * UInt32(bitsPerSample / 8)
            let header = Self.makeHeader(
                sampleRate: sampleRate, channels: channels,
                bitsPerSample: bitsPerSample, dataBytes: dataBytes
            )
            do {
                try h.seek(toOffset: 0)
                try h.write(contentsOf: header)
                try h.close()
            } catch {
                print("[WAV] close error: \(error.localizedDescription)")
            }
        }
        if let h = rawHandle {
            rawHandle = nil
            try? h.close()
        }
    }

    /// Convenience: returns the generated WAV basename.
    var filename: String { "\(basename).wav" }

    private static func makeHeader(
        sampleRate: UInt32, channels: UInt16, bitsPerSample: UInt16, dataBytes: UInt32
    ) -> Data {
        let byteRate: UInt32  = sampleRate * UInt32(channels) * UInt32(bitsPerSample / 8)
        let blockAlign: UInt16 = channels * (bitsPerSample / 8)
        let chunkSize: UInt32  = 36 + dataBytes
        var d = Data()
        d.append(Data("RIFF".utf8))
        d.append(Self.u32(chunkSize))
        d.append(Data("WAVE".utf8))
        d.append(Data("fmt ".utf8))
        d.append(Self.u32(16))
        d.append(Self.u16(1))
        d.append(Self.u16(channels))
        d.append(Self.u32(sampleRate))
        d.append(Self.u32(byteRate))
        d.append(Self.u16(blockAlign))
        d.append(Self.u16(bitsPerSample))
        d.append(Data("data".utf8))
        d.append(Self.u32(dataBytes))
        return d
    }

    private static func u32(_ v: UInt32) -> Data {
        var le = v.littleEndian
        return Data(bytes: &le, count: 4)
    }
    private static func u16(_ v: UInt16) -> Data {
        var le = v.littleEndian
        return Data(bytes: &le, count: 2)
    }
}
