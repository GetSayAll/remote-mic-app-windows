import Foundation

enum Log {
    private static let lock = NSLock()
    private static let maximumBytes: UInt64 = 5 * 1_024 * 1_024
    private static var handle: FileHandle?

    static var isEnabled: Bool { AppStorage.loggingEnabled }

    static func setEnabled(_ enabled: Bool) {
        lock.lock()
        defer { lock.unlock() }
        AppStorage.loggingEnabled = enabled
        if enabled {
            openHandleIfNeeded()
        } else {
            closeHandle()
        }
    }

    static func write(_ message: String) {
        guard AppStorage.loggingEnabled else { return }
        lock.lock()
        defer { lock.unlock() }

        rotateIfNeeded()
        openHandleIfNeeded()
        guard let handle else { return }
        let line = "[\(Date())] \(message)\n"
        try? handle.write(contentsOf: Data(line.utf8))
    }

    static func clear() {
        lock.lock()
        defer { lock.unlock() }
        closeHandle()
        try? FileManager.default.removeItem(at: AppStorage.logFile)
        if AppStorage.loggingEnabled {
            openHandleIfNeeded()
        }
    }

    static var byteSize: UInt64 {
        let values = try? AppStorage.logFile.resourceValues(
            forKeys: [.fileSizeKey]
        )
        return UInt64(values?.fileSize ?? 0)
    }

    private static func openHandleIfNeeded() {
        guard handle == nil else { return }
        AppStorage.ensureDirectory(AppStorage.logsDirectory)
        if !FileManager.default.fileExists(atPath: AppStorage.logFile.path) {
            FileManager.default.createFile(
                atPath: AppStorage.logFile.path,
                contents: nil
            )
        }
        handle = try? FileHandle(forWritingTo: AppStorage.logFile)
        _ = try? handle?.seekToEnd()
    }

    private static func rotateIfNeeded() {
        guard byteSize >= maximumBytes else { return }
        closeHandle()
        try? Data().write(to: AppStorage.logFile)
    }

    private static func closeHandle() {
        try? handle?.close()
        handle = nil
    }
}

func print(_ message: String) {
    Log.write(message)
}
