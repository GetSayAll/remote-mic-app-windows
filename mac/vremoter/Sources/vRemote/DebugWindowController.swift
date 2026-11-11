import AppKit
import ApplicationServices
import AVFoundation
import CoreBluetooth
import CoreGraphics
import SwiftUI

enum PermissionKind: String, Identifiable, CaseIterable {
    case doubaoInput
    case microphone
    case accessibility
    case inputMonitoring
    case bluetooth

    var id: String { rawValue }

    var title: String {
        switch self {
        case .doubaoInput:
            VoiceInputMethod.current == .wetype
                ? L10n.text("微信输入法麦克风设置", "WeType Microphone Setup")
                : L10n.text("豆包麦克风设置", "Doubao Microphone Setup")
        case .microphone: L10n.text("麦克风权限", "Microphone Permission")
        case .accessibility: L10n.text("辅助功能权限", "Accessibility Permission")
        case .inputMonitoring: L10n.text("输入监控权限", "Input Monitoring Permission")
        case .bluetooth: L10n.text("蓝牙权限", "Bluetooth Permission")
        }
    }

    var settingsURL: URL? {
        let anchor: String
        switch self {
        case .doubaoInput: return nil
        case .microphone: anchor = "Privacy_Microphone"
        case .accessibility: anchor = "Privacy_Accessibility"
        case .inputMonitoring: anchor = "Privacy_ListenEvent"
        case .bluetooth: anchor = "Privacy_Bluetooth"
        }
        return URL(
            string: "x-apple.systempreferences:com.apple.preference.security?\(anchor)"
        )
    }

    var screenshotNames: [String] {
        switch self {
        case .doubaoInput where VoiceInputMethod.current == .wetype:
            // No bundled screenshots yet; the guide shows its placeholder.
            ["wetype-input0", "wetype-input1"]
        case .doubaoInput:
            [
                "permission-app-management",
                "doubaoinput0",
                "doubaoinput1",
                "doubaoinput2"
            ]
        case .microphone: ["permission-microphone", "permission-microphone"]
        case .accessibility: ["permission-accessibility", "permission-accessibility"]
        case .inputMonitoring: ["permission-input-monitoring", "permission-input-monitoring"]
        case .bluetooth: ["permission-bluetooth", "permission-bluetooth"]
        }
    }

    var guidance: [String] {
        switch self {
        case .doubaoInput where VoiceInputMethod.current == .wetype:
            return [
                L10n.text(
                    "微信输入法使用系统输入设备。点击下方按钮，vRemoter 会把系统输入切换为 vRemoteDr 2ch，并打开“声音 → 输入”供你确认。",
                    "WeType records from the system input. The button below switches the system input to vRemoteDr 2ch and opens Sound > Input so you can confirm."
                ),
                L10n.text(
                    "如果在微信输入法的语音设置里单独选过麦克风，请改回“跟随系统”或选择 vRemoteDr 2ch。",
                    "If WeType's voice settings pin a specific microphone, switch it back to Follow System or choose vRemoteDr 2ch."
                )
            ]
        case .doubaoInput:
            return [
                L10n.text(
                    "首次直接打开豆包设置时，macOS 可能询问“App 管理”；请允许 vRemote 启动豆包输入法的设置组件。",
                    "The first direct launch may ask for App Management. Allow vRemote to open Doubao's settings component."
                ),
                L10n.text(
                    "打开菜单栏输入法菜单，点击“豆包输入法设置”。",
                    "Open the input menu in the menu bar and choose Doubao Input Method Settings."
                ),
                L10n.text(
                    "进入“语音输入”，找到“麦克风选择”，点击当前选项。",
                    "Open Voice Input, locate Microphone Selection, and click the current option."
                ),
                L10n.text(
                    "选择“自动检测”或“vRemoteDr 2ch”。如果自动检测没有声音，请直接选择 vRemoteDr 2ch。",
                    "Choose Automatic Detection or vRemoteDr 2ch. If automatic detection is silent, select vRemoteDr 2ch directly."
                )
            ]
        case .microphone:
            return [
                L10n.text(
                    "系统设置会打开到“隐私与安全性 → 麦克风”。",
                    "System Settings will open Privacy & Security > Microphone."
                ),
                L10n.text(
                    "找到 vRemote（或 vRemoter），打开右侧开关；若已经打开，关闭后重新打开一次。",
                    "Find vRemote or vRemoter and enable it. If already enabled, turn it off and on once."
                )
            ]
        case .accessibility:
            return [
                L10n.text(
                    "系统设置会打开到“隐私与安全性 → 辅助功能”。",
                    "System Settings will open Privacy & Security > Accessibility."
                ),
                L10n.text(
                    "找到 vRemote（或 vRemoter）并打开开关。列表中没有时，点加号选择应用。",
                    "Enable vRemote or vRemoter. If it is missing, use the plus button to add the app."
                )
            ]
        case .inputMonitoring:
            return [
                L10n.text(
                    "系统设置会打开到“隐私与安全性 → 输入监控”。",
                    "System Settings will open Privacy & Security > Input Monitoring."
                ),
                L10n.text(
                    "找到 vRemote（或 vRemoter）并打开开关。macOS 提示重新启动时允许它重新打开。",
                    "Enable vRemote or vRemoter, then allow macOS to relaunch it when prompted."
                )
            ]
        case .bluetooth:
            return [
                L10n.text(
                    "系统设置会打开到“隐私与安全性 → 蓝牙”。",
                    "System Settings will open Privacy & Security > Bluetooth."
                ),
                L10n.text(
                    "找到 vRemote（或 vRemoter）并打开开关，然后返回应用等待遥控器重新连接。",
                    "Enable vRemote or vRemoter, then return to the app and wait for the remote to reconnect."
                )
            ]
        }
    }

    var pageCount: Int { screenshotNames.count }

    func screenshotName(for page: Int) -> String {
        screenshotNames[min(max(page, 0), pageCount - 1)]
    }
}

private enum ConsoleTheme {
    static let panel = Color(red: 0.055, green: 0.059, blue: 0.067)
    static let surface = Color(red: 0.083, green: 0.091, blue: 0.103)
    static let surface2 = Color(red: 0.102, green: 0.112, blue: 0.124)
    static let line = Color(red: 0.22, green: 0.23, blue: 0.25)
    static let lineSoft = Color(red: 0.15, green: 0.16, blue: 0.18)
    static let text = Color(red: 0.93, green: 0.94, blue: 0.95)
    static let secondary = Color(red: 0.55, green: 0.57, blue: 0.61)
    static let tertiary = Color(red: 0.39, green: 0.41, blue: 0.45)
    static let green = Color(red: 0.31, green: 0.82, blue: 0.53)
    static let amber = Color(red: 0.96, green: 0.61, blue: 0.19)
    static let amberDeep = Color(red: 0.16, green: 0.13, blue: 0.09)
    static let red = Color(red: 0.94, green: 0.33, blue: 0.34)
    static let redDeep = Color(red: 0.25, green: 0.08, blue: 0.09)
    static let black = Color(red: 0.025, green: 0.028, blue: 0.032)
}

enum LogoAsset {
    static let image: NSImage = {
        let candidates: [URL?] = [
            Bundle.main.url(forResource: "vRemoterLogo", withExtension: "png"),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                .appendingPathComponent("Design/vRemoter-Logo-v1/vRemoter-app-icon-v9.png")
        ]
        for candidate in candidates.compactMap({ $0 }) {
            if let image = NSImage(contentsOf: candidate) { return image }
        }
        return NSImage(size: NSSize(width: 64, height: 64))
    }()
}

private enum GuideAsset {
    static func image(named name: String) -> NSImage? {
        let candidates: [URL?] = [
            Bundle.main.resourceURL?
                .appendingPathComponent("PermissionGuides", isDirectory: true)
                .appendingPathComponent("\(name).png"),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                .appendingPathComponent("Resources/PermissionGuides/\(name).png")
        ]
        for candidate in candidates.compactMap({ $0 }) {
            if let image = NSImage(contentsOf: candidate) { return image }
        }
        return nil
    }
}

private enum RemoteImageAsset {
    static func image(for remote: SupportedRemoteID) -> NSImage? {
        let name = remote == .chromecast
            ? "chromecast-voice-remote"
            : "x6-remote"
        let candidates: [URL?] = [
            Bundle.main.resourceURL?
                .appendingPathComponent("RemoteImages", isDirectory: true)
                .appendingPathComponent("\(name).png"),
            URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
                .appendingPathComponent("Resources/RemoteImages/\(name).png")
        ]
        for candidate in candidates.compactMap({ $0 }) {
            if let image = NSImage(contentsOf: candidate) { return image }
        }
        return nil
    }
}

private enum ConsoleModal: Identifiable {
    case permission(PermissionKind)
    case donationPrompt
    case donation
    case purchase

    var id: String {
        switch self {
        case .permission(let kind): "permission-\(kind.id)"
        case .donationPrompt: "donation-prompt"
        case .donation: "donation"
        case .purchase: "purchase"
        }
    }
}

private enum ConsolePage: String, CaseIterable, Identifiable {
    case mixer
    case mapping

    var id: String { rawValue }

    var title: String {
        switch self {
        case .mixer: L10n.text("音频", "Audio")
        case .mapping: L10n.text("按键映射", "Key Mapping")
        }
    }

    var symbol: String {
        switch self {
        case .mixer: "slider.horizontal.3"
        case .mapping: "keyboard"
        }
    }
}

@MainActor
private final class ConsoleViewModel: ObservableObject {
    @Published var selectedPage: ConsolePage = CommandLine.arguments.contains(
        "--mapping-window-demo"
    ) ? .mapping : .mixer
    @Published var status = "启动中"
    @Published var hidConnected = false
    @Published var bleConnected = false
    @Published var remoteStreaming = false
    @Published var macInputEnabled = true
    @Published var remoteInputEnabled = true
    @Published var doubaoIsRecording = false
    @Published var doubaoInput = "--"
    @Published var driverAvailable = false
    @Published var macLevelDB = -120.0
    @Published var remoteLevelDB = -120.0
    @Published var microphoneGranted = false
    @Published var accessibilityGranted = false
    @Published var inputMonitoringGranted = false
    @Published var bluetoothGranted = false
    @Published var x6Connected = false
    @Published var chromecastConnected = false
    @Published var inputTriggerKey = AppStorage.inputTriggerKey
    @Published var voiceInputMethod = VoiceInputMethod.current
    @Published var activeModal: ConsoleModal?
    @Published var showX6MicHint = !UserDefaults.standard.bool(
        forKey: "vRemoter.hasCompletedX6MicTest"
    )

    private var donationPromptShownInSession = false

    var onStopMicrophone: (() -> Void)?
    var onRestartApp: (() -> Void)?
    var onMacInputEnabledChanged: ((Bool) -> Void)?
    var onRemoteInputEnabledChanged: ((Bool) -> Void)?
    var onInputTriggerChanged: (() -> Void)?
    var onVoiceInputMethodChanged: ((VoiceInputMethod) -> Void)?
    var onRemoteMappingEnabledChanged: ((SupportedRemoteID, Bool) -> Void)?

    var doubaoUsesVRemote: Bool { doubaoInput.contains("vRemoteDr 2ch") }
    var macSolo: Bool { macInputEnabled && !remoteInputEnabled }
    var remoteSolo: Bool { remoteInputEnabled && !macInputEnabled }

    func setInputTrigger(_ trigger: InputTriggerKey) {
        guard inputTriggerKey != trigger else { return }
        inputTriggerKey = trigger
        AppStorage.inputTriggerKey = trigger
        onInputTriggerChanged?()
    }

    func setVoiceInputMethod(_ method: VoiceInputMethod) {
        guard voiceInputMethod != method else { return }
        voiceInputMethod = method
        VoiceInputMethod.select(method)
        onVoiceInputMethodChanged?(method)
    }

    func setRemoteMappingEnabled(_ enabled: Bool, remote: SupportedRemoteID) {
        RemoteMappingStore.shared.setEnabled(enabled, for: remote)
        onRemoteMappingEnabledChanged?(remote, enabled)
    }

    func toggleMacMute() {
        let next = !macInputEnabled
        macInputEnabled = next
        onMacInputEnabledChanged?(next)
    }

    func toggleRemoteMute() {
        let next = !remoteInputEnabled
        remoteInputEnabled = next
        onRemoteInputEnabledChanged?(next)
    }

    func toggleMacSolo() {
        let shouldSolo = !macSolo
        macInputEnabled = true
        remoteInputEnabled = !shouldSolo
        onMacInputEnabledChanged?(true)
        onRemoteInputEnabledChanged?(!shouldSolo)
    }

    func toggleRemoteSolo() {
        let shouldSolo = !remoteSolo
        remoteInputEnabled = true
        macInputEnabled = !shouldSolo
        onRemoteInputEnabledChanged?(true)
        onMacInputEnabledChanged?(!shouldSolo)
    }

    func refreshPermissions() {
        let demoMode = ProcessInfo.processInfo.environment["VREMOTER_PERMISSION_DEMO"] == "1"
            || CommandLine.arguments.contains("--permission-demo")
        if demoMode {
            microphoneGranted = false
            accessibilityGranted = false
            inputMonitoringGranted = false
            bluetoothGranted = false
            return
        }
        microphoneGranted = AVCaptureDevice.authorizationStatus(for: .audio) == .authorized
        accessibilityGranted = AXIsProcessTrusted()
        inputMonitoringGranted = CGPreflightListenEventAccess()
        if #available(macOS 11.0, *) {
            bluetoothGranted = CBManager.authorization == .allowedAlways
        } else {
            bluetoothGranted = true
        }
        evaluateDonationPrompt()
    }

    func openSettings(for kind: PermissionKind) {
        if kind == .doubaoInput {
            if voiceInputMethod.followsSystemDefaultInput {
                SystemInputRoute.routeToVRemote()
                openSoundInputSettings()
            } else {
                openDoubaoSettings()
            }
            activeModal = nil
            return
        }
        guard let url = kind.settingsURL else { return }
        NSWorkspace.shared.open(url)
        activeModal = nil
    }

    private func openDoubaoSettings() {
        let bundleIdentifier = "com.bytedance.inputmethod.doubaoime.settings"
        let candidates = [
            NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleIdentifier),
            URL(
                fileURLWithPath:
                    "/Library/Input Methods/DoubaoIme.app/Contents/DoubaoImeSettings.app"
            )
        ]
        for candidate in candidates.compactMap({ $0 })
            where FileManager.default.fileExists(atPath: candidate.path) {
            if NSWorkspace.shared.open(candidate) { return }
        }
    }

    func openSoundInputSettings() {
        let candidates = [
            "x-apple.systempreferences:com.apple.Sound-Settings.extension?input",
            "x-apple.systempreferences:com.apple.preference.sound?input"
        ]
        for value in candidates {
            if let url = URL(string: value), NSWorkspace.shared.open(url) { break }
        }
    }

    func showDonation(manual: Bool = true) {
        if manual { DonationPromptSchedule.markHandled() }
        activeModal = .donation
    }

    func showPurchase() {
        CommerceConfigModel.shared.refresh()
        AppAnalytics.signal("Commerce.opened")
        activeModal = .purchase
    }

    func showDonationFromPrompt() {
        DonationPromptSchedule.markHandled()
        activeModal = nil
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [weak self] in
            self?.activeModal = .donation
        }
    }

    func dismissDonationPrompt() {
        DonationPromptSchedule.markHandled()
        activeModal = nil
    }

    func observeRemoteLevel(_ db: Double) {
        remoteLevelDB = db
        guard showX6MicHint, db > -100 else { return }
        UserDefaults.standard.set(true, forKey: "vRemoter.hasCompletedX6MicTest")
        showX6MicHint = false
    }

    private func evaluateDonationPrompt() {
        guard !donationPromptShownInSession else { return }
        if CommandLine.arguments.contains("--donation-prompt-demo") {
            presentDonationPrompt()
            return
        }
        let doubaoHealthy = !doubaoIsRecording || doubaoUsesVRemote
        let allHealthy = hidConnected
            && bleConnected
            && driverAvailable
            && microphoneGranted
            && accessibilityGranted
            && inputMonitoringGranted
            && doubaoHealthy
        guard DonationPromptSchedule.shouldPresent(isHealthy: allHealthy) else { return }
        presentDonationPrompt()
    }

    private func presentDonationPrompt() {
        donationPromptShownInSession = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) { [weak self] in
            self?.activeModal = .donationPrompt
        }
    }
}

final class DebugWindowController: NSWindowController, NSWindowDelegate {
    private static let windowSize = NSSize(width: 775, height: 658)

    var onStopMicrophone: (() -> Void)?
    var onRestartApp: (() -> Void)?
    var onMacInputEnabledChanged: ((Bool) -> Void)?
    var onRemoteInputEnabledChanged: ((Bool) -> Void)?
    var onInputTriggerChanged: (() -> Void)?
    var onVoiceInputMethodChanged: ((VoiceInputMethod) -> Void)?
    var onRemoteMappingEnabledChanged: ((SupportedRemoteID, Bool) -> Void)?

    private let model = ConsoleViewModel()
    private var permissionTimer: Timer?

    init() {
        let windowSize = Self.windowSize
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: windowSize),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.title = L10n.text("vRemoter 控制台", "vRemoter Console")
        window.titleVisibility = .hidden
        window.titlebarAppearsTransparent = true
        window.isMovableByWindowBackground = true
        window.center()
        window.isReleasedWhenClosed = false
        window.backgroundColor = NSColor(
            red: 0.055,
            green: 0.059,
            blue: 0.067,
            alpha: 1
        )
        window.appearance = NSAppearance(named: .darkAqua)
        window.minSize = windowSize
        window.maxSize = windowSize
        super.init(window: window)
        window.delegate = self

        model.onStopMicrophone = { [weak self] in self?.onStopMicrophone?() }
        model.onRestartApp = { [weak self] in self?.onRestartApp?() }
        model.onMacInputEnabledChanged = { [weak self] enabled in
            self?.onMacInputEnabledChanged?(enabled)
        }
        model.onRemoteInputEnabledChanged = { [weak self] enabled in
            self?.onRemoteInputEnabledChanged?(enabled)
        }
        model.onInputTriggerChanged = { [weak self] in
            self?.onInputTriggerChanged?()
        }
        model.onVoiceInputMethodChanged = { [weak self] method in
            self?.onVoiceInputMethodChanged?(method)
        }
        model.onRemoteMappingEnabledChanged = { [weak self] remote, enabled in
            self?.onRemoteMappingEnabledChanged?(remote, enabled)
        }
        window.contentView = NSHostingView(
            rootView: StudioMixerView(model: model)
                .preferredColorScheme(.dark)
                .frame(
                    width: windowSize.width,
                    height: windowSize.height
                )
        )
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func show() {
        guard let window else { return }
        model.refreshPermissions()
        startPermissionTimer()
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
    }

    func update(
        status: String,
        hidConnected: Bool,
        bleConnected: Bool,
        remoteStreaming: Bool,
        macInputEnabled: Bool,
        remoteInputEnabled: Bool,
        doubaoIsRecording: Bool,
        doubaoInput: String,
        driverAvailable: Bool,
        x6Connected: Bool,
        chromecastConnected: Bool,
        macLevelDB: Double? = nil,
        remoteLevelDB: Double? = nil
    ) {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.model.status = status
            self.model.hidConnected = hidConnected
            self.model.bleConnected = bleConnected
            self.model.remoteStreaming = remoteStreaming
            self.model.macInputEnabled = macInputEnabled
            self.model.remoteInputEnabled = remoteInputEnabled
            self.model.doubaoIsRecording = doubaoIsRecording
            self.model.doubaoInput = doubaoInput
            self.model.driverAvailable = driverAvailable
            self.model.x6Connected = x6Connected
            self.model.chromecastConnected = chromecastConnected
            if let macLevelDB { self.model.macLevelDB = macLevelDB }
            if let remoteLevelDB { self.model.remoteLevelDB = remoteLevelDB }
            self.model.refreshPermissions()
        }
    }

    func updateMacLevel(_ db: Double) {
        DispatchQueue.main.async { [weak self] in self?.model.macLevelDB = db }
    }

    func updateRemoteLevel(_ db: Double) {
        DispatchQueue.main.async { [weak self] in self?.model.observeRemoteLevel(db) }
    }

    func showDonation() {
        model.showDonation()
        show()
    }

    func showPurchase() {
        model.showPurchase()
        show()
    }

    func windowWillClose(_ notification: Notification) {
        permissionTimer?.invalidate()
        permissionTimer = nil
    }

    private func startPermissionTimer() {
        permissionTimer?.invalidate()
        permissionTimer = Timer.scheduledTimer(withTimeInterval: 1.5, repeats: true) {
            [weak self] _ in
            Task { @MainActor [weak self] in
                self?.model.refreshPermissions()
            }
        }
    }
}

private struct StudioMixerView: View {
    @ObservedObject var model: ConsoleViewModel
    @ObservedObject private var commerce = CommerceConfigModel.shared

    var body: some View {
        VStack(spacing: 0) {
            header
                .frame(height: 68)
            if model.selectedPage == .mixer {
                Spacer().frame(height: 15)
                mixer
                    .frame(height: 288)
                Spacer().frame(height: 17)
                statusPanel
                    .frame(height: 230)
            } else {
                Spacer().frame(height: 15)
                KeyMappingView(model: model)
                    .frame(height: 535)
            }
        }
        .padding(20)
        .frame(width: 775, height: 658)
        .background(ConsoleTheme.panel)
        .overlay(alignment: .bottom) {
            supportLink
                .padding(.bottom, 8)
        }
        .sheet(item: $model.activeModal) { modal in
            switch modal {
            case .permission(let kind):
                PermissionGuideView(
                    kind: kind,
                    onCancel: { model.activeModal = nil },
                    onOpenSettings: { model.openSettings(for: kind) }
                )
            case .donationPrompt:
                DonationPromptView(
                    onDonate: model.showDonationFromPrompt,
                    onLater: model.dismissDonationPrompt
                )
            case .donation:
                DonationView()
            case .purchase:
                PurchaseView(model: .shared)
            }
        }
        .onAppear {
            commerce.refresh()
        }
    }

    private var header: some View {
        HStack(spacing: 10) {
            Image(nsImage: LogoAsset.image)
                .resizable()
                .interpolation(.high)
                .frame(width: 45, height: 45)
            VStack(alignment: .leading, spacing: 1) {
                Text("vRemoter")
                    .font(.system(size: 24, weight: .semibold))
                    .foregroundStyle(ConsoleTheme.text)
                Text(L10n.text("双麦语音输入", "Dual microphone input"))
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(ConsoleTheme.secondary)
            }
            ConsolePageSelector(selection: $model.selectedPage)
                .frame(width: 202)
            Spacer()
            HStack(spacing: 7) {
                Circle()
                    .fill(statusColor)
                    .frame(width: 9, height: 9)
                Text(model.remoteStreaming
                    ? L10n.text("录音中", "Recording")
                    : statusText)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(statusColor)
            }
            if model.remoteStreaming {
                Button(L10n.text("结束录音", "Stop")) { model.onStopMicrophone?() }
                    .buttonStyle(ConsoleButtonStyle(
                        tone: ConsoleButtonTone.error,
                        compact: true,
                        consoleSized: true
                    ))
            }
        }
        .padding(.horizontal, 20)
        .background(panelBackground(ConsoleTheme.surface, radius: 19))
    }

    private var supportLink: some View {
        Button {
            model.showDonation()
        } label: {
            HStack(spacing: 4) {
                Image(systemName: "cup.and.saucer.fill")
                Text(L10n.text("打赏", "Buy me a coffee"))
            }
            .font(.system(size: 10.5, weight: .medium))
            .foregroundStyle(ConsoleTheme.tertiary)
            .fixedSize(horizontal: true, vertical: false)
        }
        .buttonStyle(.plain)
        .help(L10n.text("支持 vRemoter", "Support vRemoter"))
    }

    private var statusText: String {
        if !model.microphoneGranted || !model.accessibilityGranted || !model.inputMonitoringGranted {
            return L10n.text("需要处理", "Action needed")
        }
        if model.hidConnected && model.bleConnected {
            return L10n.text("正在运行", "Running")
        }
        return L10n.text("正在连接", "Connecting")
    }

    private var statusColor: Color {
        if model.remoteStreaming { return ConsoleTheme.amber }
        if !model.microphoneGranted || !model.accessibilityGranted || !model.inputMonitoringGranted {
            return ConsoleTheme.red
        }
        return model.hidConnected && model.bleConnected
            ? ConsoleTheme.green
            : ConsoleTheme.amber
    }

    private var mixer: some View {
        HStack(spacing: 0) {
            InputChannelView(
                title: L10n.text("MacBook 麦克风", "MacBook Mic"),
                subtitle: L10n.text("电脑内置", "Built in"),
                db: model.macLevelDB,
                enabled: model.macInputEnabled,
                solo: model.macSolo,
                onMute: model.toggleMacMute,
                onSolo: model.toggleMacSolo
            )
            .frame(width: 225)
            Divider().overlay(ConsoleTheme.lineSoft)
            InputChannelView(
                title: L10n.text("遥控器麦克风", "Remote Mic"),
                subtitle: L10n.text("蓝牙语音遥控器", "Bluetooth Voice Remote"),
                db: model.remoteLevelDB,
                enabled: model.remoteInputEnabled,
                solo: model.remoteSolo,
                purchaseLink: !commerce.config.availableStores.isEmpty,
                showTestHint: model.showX6MicHint && model.hidConnected && model.bleConnected,
                onPurchase: model.showPurchase,
                onMute: model.toggleRemoteMute,
                onSolo: model.toggleRemoteSolo
            )
            .frame(width: 224)
            Divider().overlay(ConsoleTheme.lineSoft)
            OutputChannelView(model: model)
                .frame(width: 284)
        }
        .background(panelBackground(ConsoleTheme.surface2, radius: 19))
        .clipped()
    }

    private var statusPanel: some View {
        VStack(spacing: 0) {
            StatusRow(
                title: L10n.text(
                    "\(model.voiceInputMethod.shortName)输入设备",
                    "\(model.voiceInputMethod.shortName) input"
                ),
                value: model.doubaoIsRecording
                    ? (model.doubaoUsesVRemote
                        ? L10n.text("vRemoteDr 2ch · 录音中", "vRemoteDr 2ch · Recording")
                        : model.doubaoInput)
                    : L10n.text("当前未录音", "Not recording"),
                tone: model.doubaoIsRecording && !model.doubaoUsesVRemote ? .error : .good,
                action: model.doubaoIsRecording && !model.doubaoUsesVRemote
                    ? L10n.text("设置方法", "Fix settings")
                    : nil,
                onAction: { model.activeModal = .permission(.doubaoInput) }
            )
            StatusRow(
                title: L10n.text("音频驱动", "Audio driver"),
                value: model.driverAvailable
                    ? L10n.text("vRemoteDr 2ch 可用", "vRemoteDr 2ch available")
                    : L10n.text("未安装或需要修复", "Missing or needs repair"),
                tone: model.driverAvailable ? .good : .error,
                action: model.driverAvailable ? nil : L10n.text("打开声音", "Open Sound"),
                onAction: model.openSoundInputSettings
            )
            StatusRow(
                title: L10n.text("遥控器连接", "Remote connection"),
                value: x6Value,
                tone: model.hidConnected && model.bleConnected ? .good : .warning,
                action: model.hidConnected && model.bleConnected
                    ? nil
                    : L10n.text("蓝牙设置", "Bluetooth settings"),
                onAction: { model.activeModal = .permission(.bluetooth) }
            )
            StatusRow(
                title: L10n.text("麦克风权限", "Microphone"),
                value: model.microphoneGranted
                    ? L10n.text("已授权", "Granted")
                    : L10n.text("未授权", "Not granted"),
                tone: model.microphoneGranted ? .good : .error,
                action: model.microphoneGranted ? nil : L10n.text("去授权", "Grant"),
                onAction: { model.activeModal = .permission(.microphone) }
            )
            StatusRow(
                title: L10n.text("辅助功能权限", "Accessibility"),
                value: model.accessibilityGranted
                    ? L10n.text("已授权", "Granted")
                    : L10n.text("语音键无法正确映射", "Voice key mapping unavailable"),
                tone: model.accessibilityGranted ? .good : .error,
                action: model.accessibilityGranted ? nil : L10n.text("去授权", "Grant"),
                onAction: { model.activeModal = .permission(.accessibility) }
            )
            StatusRow(
                title: L10n.text("输入监控权限", "Input Monitoring"),
                value: model.inputMonitoringGranted
                    ? L10n.text("已授权", "Granted")
                    : L10n.text("无法监听遥控器语音键", "Cannot monitor remote voice key"),
                tone: model.inputMonitoringGranted ? .good : .warning,
                action: model.inputMonitoringGranted ? nil : L10n.text("去授权", "Grant"),
                onAction: { model.activeModal = .permission(.inputMonitoring) }
            )
        }
        .padding(.horizontal, 15)
        .background(panelBackground(ConsoleTheme.surface, radius: 19))
    }

    private var x6Value: String {
        if model.hidConnected && model.bleConnected {
            return model.remoteStreaming
                ? L10n.text("HID · BLE · 语音流传输中", "HID · BLE · Streaming")
                : L10n.text("HID · BLE 已就绪", "HID · BLE ready")
        }
        if model.hidConnected || model.bleConnected {
            return L10n.text("正在连接语音服务", "Connecting voice service")
        }
        return L10n.text("HID 与 BLE 未连接", "HID and BLE disconnected")
    }
}

private struct ConsolePageSelector: View {
    @Binding var selection: ConsolePage

    var body: some View {
        HStack(spacing: 3) {
            ForEach(ConsolePage.allCases) { page in
                Button {
                    selection = page
                } label: {
                    HStack(spacing: 5) {
                        Image(systemName: page.symbol)
                        Text(page.title)
                    }
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(
                        selection == page
                            ? ConsoleTheme.text
                            : ConsoleTheme.secondary
                    )
                    .frame(maxWidth: .infinity, minHeight: 30)
                    .background(
                        selection == page
                            ? ConsoleTheme.surface2
                            : Color.clear
                    )
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                .accessibilityLabel(page.title)
            }
        }
        .padding(3)
        .background(ConsoleTheme.black.opacity(0.72))
        .clipShape(RoundedRectangle(cornerRadius: 11))
    }
}

private struct KeyMappingView: View {
    @ObservedObject var model: ConsoleViewModel
    @ObservedObject private var mappings = RemoteMappingStore.shared
    @State private var selectedDevice: SupportedRemoteID = .chromecast

    var body: some View {
        VStack(spacing: 14) {
            triggerPanel
                .frame(height: 116)
            HStack(spacing: 14) {
                devicePanel
                    .frame(width: 238)
                mappingPanel
            }
        }
    }

    private var triggerPanel: some View {
        HStack(spacing: 18) {
            VStack(alignment: .leading, spacing: 7) {
                HStack(spacing: 7) {
                    Image(systemName: "waveform.badge.mic")
                        .foregroundStyle(ConsoleTheme.green)
                    Text(L10n.text("语音输入法与触发键", "Voice input method & trigger"))
                        .font(.system(size: 17, weight: .semibold))
                        .foregroundStyle(ConsoleTheme.text)
                }
                Text(L10n.text(
                    "遥控器语音键与电脑键盘都使用这个按键控制\(model.voiceInputMethod.title)。这里必须与\(model.voiceInputMethod.title)中的语音快捷键保持一致。",
                    "The remote voice key and Mac keyboard both use this trigger to control \(model.voiceInputMethod.title). It must match its voice shortcut."
                ))
                    .font(.system(size: 12.5))
                    .foregroundStyle(ConsoleTheme.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 10)
            VStack(alignment: .trailing, spacing: 7) {
                Picker(
                    L10n.text("语音输入法", "Voice input method"),
                    selection: Binding(
                        get: { model.voiceInputMethod },
                        set: model.setVoiceInputMethod
                    )
                ) {
                    ForEach(VoiceInputMethod.allCases) { method in
                        Text(method.title).tag(method)
                    }
                }
                .labelsHidden()
                .frame(width: 158)
                .disabled(model.remoteStreaming)
                Picker(
                    L10n.text("触发键", "Trigger"),
                    selection: Binding(
                        get: { model.inputTriggerKey },
                        set: model.setInputTrigger
                    )
                ) {
                    ForEach(InputTriggerKey.allCases) { trigger in
                        Text(trigger.title).tag(trigger)
                    }
                }
                .labelsHidden()
                .frame(width: 158)
                .disabled(model.remoteStreaming)
                if model.inputTriggerKey == .function {
                    Label(
                        L10n.text("Fn 需在本机测试", "Test Fn on this Mac"),
                        systemImage: "exclamationmark.triangle.fill"
                    )
                    .font(.system(size: 10.5, weight: .medium))
                    .foregroundStyle(ConsoleTheme.amber)
                } else if model.remoteStreaming {
                    Text(L10n.text("录音结束后可修改", "Stop recording to change"))
                        .font(.system(size: 10.5))
                        .foregroundStyle(ConsoleTheme.tertiary)
                }
            }
        }
        .padding(.horizontal, 19)
        .background(panelBackground(ConsoleTheme.surface, radius: 17))
    }

    private var devicePanel: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(L10n.text("支持的设备", "Supported devices"))
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(ConsoleTheme.secondary)

            DeviceSelectionRow(
                title: SupportedRemoteID.chromecast.title,
                signature: SupportedRemoteID.chromecast.signature,
                connected: model.chromecastConnected,
                selected: selectedDevice == .chromecast
            ) { selectedDevice = .chromecast }

            DeviceSelectionRow(
                title: SupportedRemoteID.x6.title,
                signature: SupportedRemoteID.x6.signature,
                connected: model.x6Connected,
                selected: selectedDevice == .x6
            ) { selectedDevice = .x6 }

            Spacer()
            Label(
                L10n.text(
                    "设置跟随型号，不绑定某一只遥控器",
                    "Settings follow the model, not one unit"
                ),
                systemImage: "checkmark.shield.fill"
            )
            .font(.system(size: 10.5, weight: .medium))
            .foregroundStyle(ConsoleTheme.green)
            .fixedSize(horizontal: false, vertical: true)
        }
        .padding(15)
        .background(panelBackground(ConsoleTheme.surface, radius: 17))
    }

    private var mappingPanel: some View {
        HStack(spacing: 14) {
            VStack(spacing: 10) {
                RemoteProductImage(kind: selectedDevice)
                    .frame(width: 128)
                Text(selectedDevice == .chromecast
                    ? L10n.text("14 个可映射键", "14 mappable buttons")
                    : L10n.text("15 个正面可映射键", "15 mappable front buttons"))
                    .font(.system(size: 10.5, weight: .medium))
                    .foregroundStyle(ConsoleTheme.secondary)
                    .multilineTextAlignment(.center)
                if selectedDevice == .x6 {
                    Text(L10n.text(
                        "鼠标模式键由设备内部处理",
                        "Mouse Mode is handled by the device"
                    ))
                        .font(.system(size: 9.5))
                        .foregroundStyle(ConsoleTheme.tertiary)
                        .multilineTextAlignment(.center)
                }
                Spacer()
            }
            .padding(.top, 14)
            .frame(width: 130)
            Divider().overlay(ConsoleTheme.lineSoft)
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(selectedDevice.title)
                            .font(.system(size: 16, weight: .semibold))
                            .foregroundStyle(ConsoleTheme.text)
                        Text(L10n.text("点击右侧菜单修改按键", "Use each menu to assign a key"))
                            .font(.system(size: 10.5))
                            .foregroundStyle(ConsoleTheme.tertiary)
                    }
                    Spacer()
                    Toggle(
                        L10n.text("启用映射", "Enable"),
                        isOn: Binding(
                            get: { mappings.isEnabled(selectedDevice) },
                            set: { model.setRemoteMappingEnabled($0, remote: selectedDevice) }
                        )
                    )
                    .toggleStyle(.switch)
                    .controlSize(.small)
                }
                Divider().overlay(ConsoleTheme.lineSoft)
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(RemoteProfiles.buttons(for: selectedDevice)) { button in
                            MappingRow(
                                remote: selectedDevice,
                                button: button,
                                enabled: mappings.isEnabled(selectedDevice)
                            )
                        }
                    }
                }
                HStack {
                    if !mappings.isEnabled(selectedDevice) {
                        Label(
                            L10n.text("未启用时保持系统原行为", "System behavior is unchanged"),
                            systemImage: "info.circle"
                        )
                        .font(.system(size: 10.5))
                        .foregroundStyle(ConsoleTheme.secondary)
                    }
                    Spacer()
                    Button(L10n.text("恢复默认", "Reset")) {
                        mappings.reset(selectedDevice)
                    }
                    .buttonStyle(.plain)
                    .font(.system(size: 11.5, weight: .semibold))
                    .foregroundStyle(ConsoleTheme.green)
                }
            }
            .frame(maxWidth: .infinity)
        }
        .padding(15)
        .background(panelBackground(ConsoleTheme.surface, radius: 17))
    }
}

private struct DeviceSelectionRow: View {
    let title: String
    let signature: String
    let connected: Bool
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 7) {
                HStack(spacing: 6) {
                    Circle()
                        .fill(connected ? ConsoleTheme.green : ConsoleTheme.tertiary)
                        .frame(width: 8, height: 8)
                    Text(title)
                        .font(.system(size: 12.5, weight: .semibold))
                        .foregroundStyle(ConsoleTheme.text)
                        .lineLimit(1)
                }
                HStack {
                    Text(signature)
                        .font(.system(size: 10.5, design: .monospaced))
                        .foregroundStyle(ConsoleTheme.tertiary)
                    Spacer()
                    Text(connected
                        ? L10n.text("已连接", "Connected")
                        : L10n.text("未连接", "Offline"))
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(connected ? ConsoleTheme.green : ConsoleTheme.tertiary)
                }
            }
            .padding(11)
            .background(selected ? ConsoleTheme.surface2 : ConsoleTheme.black.opacity(0.35))
            .overlay(
                RoundedRectangle(cornerRadius: 11)
                    .stroke(selected ? ConsoleTheme.green.opacity(0.8) : ConsoleTheme.lineSoft, lineWidth: 1)
            )
            .clipShape(RoundedRectangle(cornerRadius: 11))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(title), \(connected ? "connected" : "offline")")
    }
}

private struct MappingRow: View {
    @ObservedObject private var mappings = RemoteMappingStore.shared
    @State private var showRecorder = false
    let remote: SupportedRemoteID
    let button: RemoteButtonDefinition
    let enabled: Bool

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: button.symbol)
                .frame(width: 18)
                .foregroundStyle(button.voiceControlled ? ConsoleTheme.green : ConsoleTheme.secondary)
            Text(button.title)
                .font(.system(size: 11.5, weight: .medium))
                .foregroundStyle(ConsoleTheme.text)
                .frame(width: 76, alignment: .leading)
            Spacer(minLength: 4)
            if !button.remappable {
                Text(L10n.text("设备内部功能", "Device-only function"))
                    .font(.system(size: 11))
                    .foregroundStyle(ConsoleTheme.tertiary)
            } else if button.voiceControlled {
                Text(RemoteMappingTarget.doubaoVoice.title)
                    .font(.system(size: 11))
                    .foregroundStyle(ConsoleTheme.green)
            } else {
                HStack(spacing: 4) {
                    Picker(
                        button.title,
                        selection: Binding(
                            get: { mappings.target(for: button, remote: remote) },
                            set: { target in
                                if target == .custom {
                                    showRecorder = true
                                } else {
                                    mappings.setTarget(target, for: button, remote: remote)
                                }
                            }
                        )
                    ) {
                        ForEach(RemoteMappingTarget.allCases.filter { $0 != .doubaoVoice }) { target in
                            Text(
                                target == .custom
                                    ? mappings.targetTitle(for: button, remote: remote)
                                    : target.title
                            ).tag(target)
                        }
                    }
                    .labelsHidden()
                    .frame(width: 142)
                    if mappings.target(for: button, remote: remote) == .custom {
                        Button { showRecorder = true } label: {
                            Image(systemName: "record.circle")
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(ConsoleTheme.green)
                        .help(L10n.text("重新录制按键", "Record again"))
                    }
                }
            }
        }
        .frame(height: 28)
        .padding(.horizontal, 9)
        .background(ConsoleTheme.surface2)
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .combine)
        .sheet(isPresented: $showRecorder) {
            KeyboardShortcutCaptureView(
                buttonTitle: button.title,
                onCancel: { showRecorder = false },
                onSave: { shortcut in
                    mappings.setCustomShortcut(
                        shortcut,
                        for: button,
                        remote: remote
                    )
                    showRecorder = false
                }
            )
        }
    }
}

private struct KeyboardShortcutCaptureView: View {
    let buttonTitle: String
    let onCancel: () -> Void
    let onSave: (RemoteCustomShortcut) -> Void
    @State private var captured: RemoteCustomShortcut?

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 5) {
                Text(L10n.text("录制键盘按键", "Record Keyboard Key"))
                    .font(.system(size: 20, weight: .semibold))
                Text(L10n.text(
                    "为“\(buttonTitle)”按下一个按键或组合键。",
                    "Press a key or shortcut for “\(buttonTitle)”."
                ))
                    .font(.system(size: 13))
                    .foregroundStyle(ConsoleTheme.secondary)
            }

            KeyboardEventCaptureView { shortcut in
                captured = shortcut
            }
            .frame(height: 82)
            .overlay(
                RoundedRectangle(cornerRadius: 13)
                    .stroke(
                        captured == nil ? ConsoleTheme.line : ConsoleTheme.green,
                        lineWidth: 1.5
                    )
            )
            .overlay {
                VStack(spacing: 5) {
                    Text(captured?.label ?? L10n.text("现在按下键盘按键", "Press a key now"))
                        .font(.system(size: 18, weight: .semibold, design: .rounded))
                        .foregroundStyle(captured == nil ? ConsoleTheme.secondary : ConsoleTheme.text)
                    Text(L10n.text("支持 Command / Option / Control / Shift 组合", "Command / Option / Control / Shift are supported"))
                        .font(.system(size: 10.5))
                        .foregroundStyle(ConsoleTheme.tertiary)
                }
                .allowsHitTesting(false)
            }

            HStack {
                Button(L10n.text("取消", "Cancel"), action: onCancel)
                Spacer()
                Button(L10n.text("保存映射", "Save Mapping")) {
                    if let captured { onSave(captured) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(captured == nil)
            }
        }
        .padding(24)
        .frame(width: 430, height: 245)
        .background(ConsoleTheme.panel)
        .foregroundStyle(ConsoleTheme.text)
    }
}

private struct KeyboardEventCaptureView: NSViewRepresentable {
    let onCapture: (RemoteCustomShortcut) -> Void

    func makeNSView(context: Context) -> KeyboardCaptureNSView {
        let view = KeyboardCaptureNSView()
        view.onCapture = onCapture
        DispatchQueue.main.async {
            view.window?.makeFirstResponder(view)
        }
        return view
    }

    func updateNSView(_ nsView: KeyboardCaptureNSView, context: Context) {
        nsView.onCapture = onCapture
        DispatchQueue.main.async {
            nsView.window?.makeFirstResponder(nsView)
        }
    }
}

private final class KeyboardCaptureNSView: NSView {
    var onCapture: ((RemoteCustomShortcut) -> Void)?

    override var acceptsFirstResponder: Bool { true }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.window?.makeFirstResponder(self)
        }
    }

    override func keyDown(with event: NSEvent) {
        guard !event.isARepeat else { return }
        onCapture?(Self.shortcut(from: event))
    }

    private static func shortcut(from event: NSEvent) -> RemoteCustomShortcut {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        var flags: CGEventFlags = []
        var prefix = ""
        if modifiers.contains(.control) {
            flags.insert(.maskControl)
            prefix += "⌃"
        }
        if modifiers.contains(.option) {
            flags.insert(.maskAlternate)
            prefix += "⌥"
        }
        if modifiers.contains(.shift) {
            flags.insert(.maskShift)
            prefix += "⇧"
        }
        if modifiers.contains(.command) {
            flags.insert(.maskCommand)
            prefix += "⌘"
        }

        let special: [UInt16: String] = [
            0x24: "Return", 0x30: "Tab", 0x31: "Space", 0x33: "Delete",
            0x35: "Esc", 0x73: "Home", 0x77: "End", 0x74: "Page Up",
            0x79: "Page Down", 0x7B: "←", 0x7C: "→", 0x7D: "↓", 0x7E: "↑",
            0x7A: "F1", 0x78: "F2", 0x63: "F3", 0x76: "F4",
            0x60: "F5", 0x61: "F6", 0x62: "F7", 0x64: "F8",
            0x65: "F9", 0x6D: "F10", 0x67: "F11", 0x6F: "F12",
        ]
        let key = special[event.keyCode]
            ?? event.charactersIgnoringModifiers?.uppercased()
            ?? String(format: "Key 0x%02X", event.keyCode)
        return RemoteCustomShortcut(
            keyCode: event.keyCode,
            flags: flags.rawValue,
            label: prefix + key
        )
    }
}

private struct RemoteProductImage: View {
    let kind: SupportedRemoteID

    var body: some View {
        Group {
            if let image = RemoteImageAsset.image(for: kind) {
                if kind == .chromecast {
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 112, height: 304)
                } else {
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 92, height: 260)
                }
            } else {
                Label(
                    L10n.text("遥控器图片缺失", "Remote image unavailable"),
                    systemImage: "photo.badge.exclamationmark"
                )
                .font(.system(size: 10.5, weight: .medium))
                .foregroundStyle(ConsoleTheme.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(height: kind == .chromecast ? 304 : 260)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(kind.title)
    }
}

private struct InputChannelView: View {
    let title: String
    let subtitle: String
    let db: Double
    let enabled: Bool
    let solo: Bool
    var purchaseLink = false
    var showTestHint = false
    var onPurchase: (() -> Void)?
    let onMute: () -> Void
    let onSolo: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.system(size: 19, weight: .semibold))
                .foregroundStyle(enabled ? ConsoleTheme.text : ConsoleTheme.tertiary)
                .lineLimit(1)
            Text(subtitle)
                .font(.system(size: 13))
                .foregroundStyle(enabled ? ConsoleTheme.secondary : ConsoleTheme.tertiary)
                .padding(.top, 4)
            HStack(spacing: 6) {
                Circle()
                    .fill(enabled ? (solo ? ConsoleTheme.amber : ConsoleTheme.green) : ConsoleTheme.tertiary)
                    .frame(width: 10, height: 10)
                Text(enabled
                    ? (solo
                        ? L10n.text("正在独奏", "Solo")
                        : L10n.text("有声音", "Active"))
                    : L10n.text("不参与混合", "Muted"))
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(enabled ? (solo ? ConsoleTheme.amber : ConsoleTheme.secondary) : ConsoleTheme.tertiary)
                Spacer()
                Text(dbText)
                    .font(.system(size: 12, weight: .medium, design: .monospaced))
                    .foregroundStyle(ConsoleTheme.secondary)
            }
            .padding(.top, 15)
            MeterView(db: enabled ? db : -120, color: ConsoleTheme.green, count: 10)
                .padding(.top, showTestHint ? 35 : 15)
                .overlay(alignment: .topLeading) {
                    if showTestHint {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.down")
                            Text(L10n.text(
                                "按一下遥控器麦克风键测试",
                                "Press the remote mic button"
                            ))
                                .lineLimit(1)
                        }
                        .font(.system(size: 9.5, weight: .semibold))
                        .foregroundStyle(ConsoleTheme.text)
                        .padding(.horizontal, 9)
                        .frame(height: 25)
                        .background(ConsoleTheme.black.opacity(0.96))
                        .clipShape(RoundedRectangle(cornerRadius: 7))
                        .overlay(
                            RoundedRectangle(cornerRadius: 7)
                                .stroke(ConsoleTheme.green.opacity(0.8), lineWidth: 1)
                        )
                        .offset(y: -1)
                    }
                }
            HStack {
                Text(L10n.text("低", "Low"))
                Spacer()
                Text(L10n.text("高", "High"))
            }
            .font(.system(size: 11))
            .foregroundStyle(ConsoleTheme.tertiary)
            .frame(width: 141)
            .padding(.top, 6)
            HStack(spacing: 8) {
                Button(L10n.text("静音", "Mute"), action: onMute)
                    .buttonStyle(ConsoleButtonStyle(
                        tone: enabled ? .neutral : .error,
                        consoleSized: true
                    ))
                Button(L10n.text("独奏", "Solo"), action: onSolo)
                    .buttonStyle(ConsoleButtonStyle(
                        tone: solo ? .warning : .neutral,
                        consoleSized: true
                    ))
            }
            .padding(.top, 31)
            HStack(spacing: 4) {
                Text(enabled
                    ? (solo
                        ? L10n.text("仅此路参与", "Solo input")
                        : L10n.text("参与混合", "In mix"))
                    : L10n.text("不参与混合", "Not in mix"))
                Spacer()
                if purchaseLink {
                    Button(L10n.text("购买遥控器 ↗", "Buy remote ↗")) {
                        onPurchase?()
                    }
                        .buttonStyle(.plain)
                        .foregroundStyle(ConsoleTheme.green)
                }
            }
            .font(.system(size: 10, weight: .medium))
            .foregroundStyle(enabled ? ConsoleTheme.secondary : ConsoleTheme.tertiary)
            .padding(.top, 22)
        }
        .padding(.horizontal, 23)
        .padding(.top, 23)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(solo ? ConsoleTheme.amber.opacity(0.06) : Color.clear)
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(solo ? ConsoleTheme.amber : Color.clear, lineWidth: 1)
        )
        .opacity(enabled ? 1 : 0.58)
    }

    private var dbText: String {
        guard enabled, db > -100 else { return "−∞" }
        return String(format: "%.0f dB", db)
    }
}

private struct OutputChannelView: View {
    @ObservedObject var model: ConsoleViewModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(L10n.text("混合输出", "Mixed Output"))
                .font(.system(size: 20, weight: .semibold))
                .foregroundStyle(ConsoleTheme.text)
            Text(L10n.text(
                "发送给\(model.voiceInputMethod.shortName)",
                "Sent to \(model.voiceInputMethod.shortName)"
            ))
                .font(.system(size: 13))
                .foregroundStyle(ConsoleTheme.secondary)
                .padding(.top, 4)
            HStack(spacing: 6) {
                Circle().fill(outputError ? ConsoleTheme.red : ConsoleTheme.amber)
                    .frame(width: 10, height: 10)
                Text(outputError
                    ? L10n.text("未输出", "No output")
                    : model.remoteStreaming
                        ? L10n.text("正在输出", "Sending")
                        : L10n.text("等待录音", "Waiting"))
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(outputError ? ConsoleTheme.red : ConsoleTheme.amber)
                Spacer()
                Text(outputError ? "−∞" : "−9 dB")
                    .font(.system(size: 12, weight: .medium, design: .monospaced))
                    .foregroundStyle(ConsoleTheme.secondary)
            }
            .padding(.top, 15)
            MeterView(
                db: outputError ? -120 : max(model.macLevelDB, model.remoteLevelDB),
                color: ConsoleTheme.amber,
                count: 14
            )
            .padding(.top, 15)
            HStack {
                Text(L10n.text("低", "Low"))
                Spacer()
                Text(L10n.text("高", "High"))
            }
            .font(.system(size: 11))
            .foregroundStyle(ConsoleTheme.tertiary)
            .frame(width: 188)
            .padding(.top, 6)
            HStack(spacing: 8) {
                Circle().fill(ConsoleTheme.amber).frame(width: 10, height: 10)
                Text(L10n.text("两路已合并", "Inputs mixed"))
                    .font(.system(
                        size: AppLanguage.selected.usesEnglish ? 11 : 14,
                        weight: .semibold
                    ))
                    .foregroundStyle(ConsoleTheme.amber)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                Spacer()
                Text("vRemoteDr 2ch")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(ConsoleTheme.text)
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
            }
            .padding(.horizontal, 15)
            .frame(height: 60)
            .background(panelBackground(ConsoleTheme.surface, radius: 14))
            .padding(.top, 25)
            Text(outputError
                ? L10n.text("等待音频链路恢复", "Waiting for audio route")
                : L10n.text(
                    "\(model.voiceInputMethod.shortName)正在接收",
                    "\(model.voiceInputMethod.shortName) is receiving"
                ))
                .font(.system(size: 11))
                .foregroundStyle(outputError ? ConsoleTheme.red : ConsoleTheme.secondary)
                .padding(.top, 15)
        }
        .padding(.horizontal, 23)
        .padding(.top, 23)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(ConsoleTheme.amberDeep)
        .overlay(alignment: .top) {
            Rectangle().fill(ConsoleTheme.amber).frame(height: 3)
        }
    }

    private var outputError: Bool {
        !model.driverAvailable || (model.doubaoIsRecording && !model.doubaoUsesVRemote)
    }
}

private struct MeterView: View {
    let db: Double
    let color: Color
    let count: Int

    var body: some View {
        HStack(spacing: 4) {
            ForEach(0..<count, id: \.self) { index in
                RoundedRectangle(cornerRadius: 2)
                    .fill(index < activeCount ? color : ConsoleTheme.black)
                    .frame(width: count > 10 ? 13 : 14, height: 21)
            }
        }
    }

    private var activeCount: Int {
        guard db > -100 else { return 0 }
        return min(count, max(0, Int(((db + 60) / 60) * Double(count))))
    }
}

private struct PurchaseView: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var model: CommerceConfigModel
    @State private var selectedStoreID = ""

    private var stores: [CommerceStore] { model.config.availableStores }
    private var selectedStore: CommerceStore? {
        stores.first(where: { $0.id == selectedStoreID }) ?? stores.first
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            ZStack {
                let heroName = AppLanguage.selected.usesEnglish
                    ? "x6-vibe-coding-hero-en.png"
                    : "x6-vibe-coding-hero-v2.png"
                if let image = CommerceAsset.image(named: heroName) {
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                        .scaledToFit()
                } else {
                    ConsoleTheme.surface
                }
            }
            .frame(height: 403)
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(ConsoleTheme.line))

            if stores.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "cart")
                        .font(.system(size: 32))
                    Text(L10n.text("购买链接暂不可用", "Purchase links are temporarily unavailable"))
                }
                .foregroundStyle(ConsoleTheme.secondary)
                .frame(maxWidth: .infinity, minHeight: 210)
            } else {
                HStack(spacing: 8) {
                    ForEach(stores) { store in
                        Button {
                            selectedStoreID = store.id
                        } label: {
                            Text(store.title)
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(
                                    selectedStore?.id == store.id
                                        ? Color.white
                                        : store.brandColor
                                )
                                .frame(maxWidth: .infinity)
                                .frame(height: 38)
                                .background(
                                    selectedStore?.id == store.id
                                        ? store.brandColor
                                        : ConsoleTheme.surface
                                )
                                .clipShape(RoundedRectangle(cornerRadius: 7))
                                .overlay(
                                    RoundedRectangle(cornerRadius: 7)
                                        .stroke(
                                            selectedStore?.id == store.id
                                                ? store.brandColor
                                                : store.brandColor.opacity(0.65)
                                        )
                                )
                        }
                        .buttonStyle(.plain)
                    }
                }

                if let store = selectedStore {
                    HStack(spacing: 20) {
                        PurchaseQRCode(store: store)
                            .frame(width: 180, height: 180)
                        VStack(alignment: .leading, spacing: 12) {
                            Text(store.title)
                                .font(.system(size: 21, weight: .semibold))
                            Text(L10n.text(
                                "电脑上可直接打开；手机购买请扫描左侧二维码。",
                                "Open the store on this Mac, or scan the QR code on your phone."
                            ))
                                .font(.system(size: 12))
                                .foregroundStyle(ConsoleTheme.secondary)
                                .lineSpacing(3)
                            Spacer()
                            HStack(spacing: 10) {
                                Button(L10n.text("复制链接", "Copy Link")) {
                                    NSPasteboard.general.clearContents()
                                    NSPasteboard.general.setString(store.url, forType: .string)
                                    AppAnalytics.signal("Commerce.linkCopied", parameters: ["store": store.id])
                                }
                                .buttonStyle(ConsoleButtonStyle(tone: .neutral))
                                Button {
                                    guard let url = store.purchaseURL else { return }
                                    AppAnalytics.signal("Commerce.storeOpened", parameters: ["store": store.id])
                                    NSWorkspace.shared.open(url)
                                } label: {
                                    Text(L10n.text("立即打开", "Open Store"))
                                        .font(.system(size: 12, weight: .semibold))
                                        .foregroundStyle(Color.white)
                                        .padding(.horizontal, 14)
                                        .frame(height: 34)
                                        .background(store.brandColor)
                                        .clipShape(RoundedRectangle(cornerRadius: 7))
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .frame(maxWidth: .infinity, minHeight: 180, alignment: .leading)
                    }
                }
            }

            HStack {
                Spacer()
                Button(L10n.text("完成", "Done")) { dismiss() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(ConsoleButtonStyle(tone: .neutral))
            }
        }
        .padding(22)
        .frame(width: 760, height: 720)
        .background(ConsoleTheme.panel)
        .foregroundStyle(ConsoleTheme.text)
        .onAppear {
            if selectedStoreID.isEmpty { selectedStoreID = stores.first?.id ?? "" }
            model.refresh()
        }
        .onChange(of: model.config) { _ in
            guard stores.contains(where: { $0.id == selectedStoreID }) else {
                selectedStoreID = stores.first?.id ?? ""
                return
            }
        }
    }
}

private struct PurchaseQRCode: View {
    let store: CommerceStore

    var body: some View {
        ZStack {
            Color.white
            if let url = store.remoteQRCodeURL {
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().interpolation(.none).scaledToFit().padding(10)
                    default:
                        fallback
                    }
                }
            } else {
                fallback
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(ConsoleTheme.line))
    }

    @ViewBuilder
    private var fallback: some View {
        if let image = CommerceAsset.image(named: "\(store.id)-qr.png") {
            Image(nsImage: image)
                .resizable()
                .interpolation(.none)
                .scaledToFit()
                .padding(10)
        } else {
            Image(systemName: "qrcode")
                .font(.system(size: 48))
                .foregroundStyle(ConsoleTheme.tertiary)
        }
    }
}

private extension CommerceStore {
    var brandColor: Color {
        switch id {
        case "xiaohongshu": Color(red: 1.0, green: 0.141, blue: 0.259)
        case "douyin": Color(red: 0.145, green: 0.902, blue: 0.941)
        default: ConsoleTheme.green
        }
    }
}

private enum DonationProvider: String, CaseIterable, Identifiable {
    case wechat
    case alipay
    case paypal

    var id: String { rawValue }

    var title: String {
        switch self {
        case .wechat: L10n.text("微信", "WeChat")
        case .alipay: L10n.text("支付宝", "Alipay")
        case .paypal: "PayPal"
        }
    }

    var imageName: String {
        switch self {
        case .wechat: "IMG_5989.JPG"
        case .alipay: "IMG_5987.JPG"
        case .paypal: "IMG_5990.JPG"
        }
    }

    var brandColor: Color {
        switch self {
        case .wechat: Color(red: 0.027, green: 0.757, blue: 0.376)
        case .alipay: Color(red: 0.086, green: 0.467, blue: 1.0)
        case .paypal: Color(red: 0.0, green: 0.188, blue: 0.529)
        }
    }
}

private struct DonationPromptView: View {
    let onDonate: () -> Void
    let onLater: () -> Void

    var body: some View {
        VStack(spacing: 22) {
            Image(nsImage: LogoAsset.image)
                .resizable()
                .frame(width: 54, height: 54)
            Text(L10n.text(
                "制作不易，麻烦打个赏补偿一点我的 Token 费吧。\n谢主隆恩🙏🙏🙏～～～",
                "vRemoter is free, but apparently tokens never got that memo. If the app saves you a little time, buying me a coffee helps keep it going. My API balance sends its thanks. 🙏"
            ))
                .font(.system(size: 21, weight: .semibold))
                .foregroundStyle(ConsoleTheme.text)
                .multilineTextAlignment(.center)
                .lineSpacing(5)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 12) {
                Button(L10n.text("以后再说", "Maybe later"), action: onLater)
                    .keyboardShortcut(.cancelAction)
                    .buttonStyle(ConsoleButtonStyle(tone: .neutral))
                Button(L10n.text("立即打赏", "Buy me a coffee"), action: onDonate)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(ConsoleButtonStyle(tone: ConsoleButtonTone.good))
            }
        }
        .padding(30)
        .frame(width: 560, height: 330)
        .background(ConsoleTheme.panel)
    }
}

private struct DonationView: View {
    @Environment(\.dismiss) private var dismiss
    @State private var provider: DonationProvider = .wechat

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Image(nsImage: LogoAsset.image)
                    .resizable()
                    .frame(width: 38, height: 38)
                VStack(alignment: .leading, spacing: 2) {
                    Text(L10n.text("给 vRemoter 打个赏", "Buy vRemoter a coffee"))
                        .font(.system(size: 20, weight: .semibold))
                    Text(L10n.text("感谢你帮我补一点 Token 费", "Free app, non-free tokens."))
                        .font(.system(size: 11))
                        .foregroundStyle(ConsoleTheme.secondary)
                }
                Spacer()
            }

            HStack(spacing: 8) {
                ForEach(DonationProvider.allCases) { item in
                    Button {
                        provider = item
                    } label: {
                        Text(item.title)
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(
                                provider == item ? Color.white : item.brandColor
                            )
                            .frame(maxWidth: .infinity)
                            .frame(height: 36)
                            .background(
                                provider == item
                                    ? item.brandColor
                                    : ConsoleTheme.surface
                            )
                            .clipShape(RoundedRectangle(cornerRadius: 7))
                            .overlay(
                                RoundedRectangle(cornerRadius: 7)
                                    .stroke(
                                        provider == item
                                            ? item.brandColor
                                            : item.brandColor.opacity(0.65),
                                        lineWidth: 1
                                    )
                            )
                    }
                    .buttonStyle(.plain)
                }
            }

            ZStack {
                ConsoleTheme.black
                if let image = DonationAsset.image(named: provider.imageName) {
                    Image(nsImage: image)
                        .resizable()
                        .interpolation(.high)
                        .scaledToFit()
                        .padding(12)
                } else {
                    Text(L10n.text("收款码暂不可用", "Payment code unavailable"))
                        .foregroundStyle(ConsoleTheme.secondary)
                }
            }
            .frame(height: 390)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(ConsoleTheme.line))

            HStack {
                Text(L10n.text("请使用对应 App 扫码", "Scan with the corresponding app"))
                    .font(.system(size: 11))
                    .foregroundStyle(ConsoleTheme.secondary)
                Spacer()
                Button(L10n.text("完成", "Done")) { dismiss() }
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(ConsoleButtonStyle(tone: ConsoleButtonTone.good))
            }
        }
        .padding(24)
        .frame(width: 620, height: 570)
        .background(ConsoleTheme.panel)
        .foregroundStyle(ConsoleTheme.text)
    }
}

private enum StatusTone { case good, warning, error }

private struct StatusPill: View {
    let title: String
    let tone: StatusTone

    var body: some View {
        HStack(spacing: 8) {
            Circle().fill(color).frame(width: 8, height: 8)
            Text(title)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(color)
        }
        .padding(.horizontal, 13)
        .frame(height: 28)
        .background(background)
        .clipShape(Capsule())
        .overlay(Capsule().stroke(color.opacity(0.9), lineWidth: 1))
    }

    private var color: Color {
        switch tone {
        case .good: ConsoleTheme.green
        case .warning: ConsoleTheme.amber
        case .error: ConsoleTheme.red
        }
    }

    private var background: Color {
        switch tone {
        case .good: ConsoleTheme.black
        case .warning: ConsoleTheme.amberDeep
        case .error: ConsoleTheme.redDeep
        }
    }
}

private struct StatusRow: View {
    let title: String
    let value: String
    let tone: StatusTone
    let action: String?
    let onAction: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Circle().fill(color).frame(width: 9, height: 9)
            Text(title)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(ConsoleTheme.secondary)
                .frame(width: 145, alignment: .leading)
            Text(value)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(tone == .good ? ConsoleTheme.text : color)
                .lineLimit(1)
            Spacer(minLength: 8)
            if let action {
                Button(action, action: onAction)
                    .buttonStyle(ConsoleButtonStyle(
                        tone: tone,
                        compact: true,
                        consoleSized: true
                    ))
            }
        }
        .padding(.horizontal, 13)
        .frame(height: 35)
        .background(tone == .good ? Color.clear : color.opacity(0.07))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    private var color: Color {
        switch tone {
        case .good: ConsoleTheme.green
        case .warning: ConsoleTheme.amber
        case .error: ConsoleTheme.red
        }
    }
}

private struct PermissionGuideView: View {
    let kind: PermissionKind
    let onCancel: () -> Void
    let onOpenSettings: () -> Void
    @State private var page = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Image(nsImage: LogoAsset.image)
                    .resizable()
                    .frame(width: 34, height: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text(kind.title)
                        .font(.system(size: 19, weight: .semibold))
                    Text(L10n.text(
                        "设置向导 · 第 \(page + 1) / \(kind.pageCount) 步",
                        "Setup guide · Step \(page + 1) of \(kind.pageCount)"
                    ))
                        .font(.system(size: 11))
                        .foregroundStyle(ConsoleTheme.secondary)
                }
                Spacer()
            }

            GuideScreenshot(kind: kind, page: $page)
                .frame(height: 350)

            Text(kind.guidance[page])
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(ConsoleTheme.text)
                .fixedSize(horizontal: false, vertical: true)

            HStack {
                HStack(spacing: 6) {
                    ForEach(0..<kind.pageCount, id: \.self) { index in
                        Capsule()
                            .fill(index == page ? ConsoleTheme.green : ConsoleTheme.line)
                            .frame(width: index == page ? 18 : 7, height: 7)
                    }
                }
                Spacer()
                Button(L10n.text("取消", "Cancel"), action: onCancel)
                    .keyboardShortcut(.cancelAction)
                    .buttonStyle(ConsoleButtonStyle(tone: .neutral))
                Button(L10n.text("立即设置", "Open Settings"), action: onOpenSettings)
                    .keyboardShortcut(.defaultAction)
                    .buttonStyle(ConsoleButtonStyle(tone: ConsoleButtonTone.good))
            }
        }
        .padding(22)
        .frame(width: 620, height: 580)
        .background(ConsoleTheme.panel)
        .foregroundStyle(ConsoleTheme.text)
    }
}

private struct GuideScreenshot: View {
    let kind: PermissionKind
    @Binding var page: Int

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                if let image = GuideAsset.image(named: kind.screenshotName(for: page)) {
                    Image(nsImage: image)
                        .resizable()
                        .scaledToFit()
                        .frame(width: proxy.size.width, height: proxy.size.height)
                } else {
                    screenshotPlaceholder
                }

                if kind != .doubaoInput || page == 0 {
                    RoundedRectangle(cornerRadius: 10)
                        .stroke(ConsoleTheme.red, lineWidth: 3)
                        .frame(
                            width: proxy.size.width * highlightWidth,
                            height: proxy.size.height * highlightHeight
                        )
                        .position(
                            x: proxy.size.width * highlightX,
                            y: proxy.size.height * highlightY
                        )
                }

                HStack {
                    guideArrow(systemName: "chevron.left", enabled: page > 0) {
                        if page > 0 { page -= 1 }
                    }
                    Spacer()
                    guideArrow(systemName: "chevron.right", enabled: page < kind.pageCount - 1) {
                        if page < kind.pageCount - 1 { page += 1 }
                    }
                }
                .padding(.horizontal, 10)
            }
            .background(ConsoleTheme.black)
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).stroke(ConsoleTheme.line))
        }
    }

    private var screenshotPlaceholder: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 12) {
                Text(L10n.text("系统设置", "System Settings"))
                    .font(.system(size: 16, weight: .semibold))
                Text(L10n.text("隐私与安全性", "Privacy & Security"))
                    .foregroundStyle(ConsoleTheme.text)
                Text(kind.title.replacingOccurrences(of: "权限", with: ""))
                    .foregroundStyle(ConsoleTheme.green)
                Spacer()
            }
            .padding(20)
            .frame(width: 190, alignment: .leading)
            .background(ConsoleTheme.surface)
            VStack(alignment: .leading, spacing: 18) {
                Text(kind.title)
                    .font(.system(size: 18, weight: .semibold))
                Text(L10n.text(
                    "允许下方的应用访问此功能。",
                    "Allow the apps below to access this feature."
                ))
                    .foregroundStyle(ConsoleTheme.secondary)
                HStack {
                    Image(nsImage: LogoAsset.image).resizable().frame(width: 30, height: 30)
                    Text("vRemoter")
                    Spacer()
                    Toggle("", isOn: .constant(false)).labelsHidden()
                }
                .padding(12)
                .background(ConsoleTheme.surface2)
                .clipShape(RoundedRectangle(cornerRadius: 9))
                Spacer()
            }
            .padding(20)
        }
        .foregroundStyle(ConsoleTheme.text)
    }

    private var secondPageY: CGFloat {
        switch kind {
        case .doubaoInput: 0.5
        case .microphone: 0.92
        case .accessibility: 0.89
        case .inputMonitoring: 0.46
        case .bluetooth: 0.74
        }
    }

    private var highlightWidth: CGFloat {
        kind == .doubaoInput ? 0.36 : (page == 0 ? 0.38 : 0.45)
    }

    private var highlightHeight: CGFloat {
        kind == .doubaoInput ? 0.11 : 0.10
    }

    private var highlightX: CGFloat {
        kind == .doubaoInput ? 0.60 : (page == 0 ? 0.255 : 0.62)
    }

    private var highlightY: CGFloat {
        kind == .doubaoInput ? 0.40 : (page == 0 ? 0.25 : secondPageY)
    }

    private func guideArrow(
        systemName: String,
        enabled: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: 14, weight: .bold))
                .frame(width: 30, height: 30)
                .background(ConsoleTheme.black.opacity(0.82))
                .clipShape(Circle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(enabled ? ConsoleTheme.text : ConsoleTheme.tertiary)
        .disabled(!enabled)
    }

}

private enum ConsoleButtonTone { case neutral, good, warning, error }

private struct ConsoleButtonStyle: ButtonStyle {
    let tone: ConsoleButtonTone
    var compact = false
    var consoleSized = false

    init(tone: ConsoleButtonTone, compact: Bool = false, consoleSized: Bool = false) {
        self.tone = tone
        self.compact = compact
        self.consoleSized = consoleSized
    }

    init(tone: StatusTone, compact: Bool = false, consoleSized: Bool = false) {
        switch tone {
        case .good: self.tone = .good
        case .warning: self.tone = .warning
        case .error: self.tone = .error
        }
        self.compact = compact
        self.consoleSized = consoleSized
    }

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: fontSize, weight: .semibold))
            .foregroundStyle(foreground)
            .padding(.horizontal, horizontalPadding)
            .frame(height: buttonHeight)
            .background(configuration.isPressed ? background.opacity(0.65) : background)
            .clipShape(RoundedRectangle(cornerRadius: cornerRadius))
            .overlay(
                RoundedRectangle(cornerRadius: cornerRadius)
                    .stroke(foreground.opacity(0.85), lineWidth: 1)
            )
    }

    private var fontSize: CGFloat {
        if consoleSized { return compact ? 11 : 14 }
        return compact ? 9 : 11
    }

    private var horizontalPadding: CGFloat {
        if consoleSized { return compact ? 15 : 23 }
        return compact ? 12 : 18
    }

    private var buttonHeight: CGFloat {
        if consoleSized { return compact ? 30 : 43 }
        return compact ? 24 : 34
    }

    private var cornerRadius: CGFloat {
        if consoleSized { return compact ? 12 : 11 }
        return compact ? 10 : 9
    }

    private var foreground: Color {
        switch tone {
        case .neutral: ConsoleTheme.text
        case .good: ConsoleTheme.green
        case .warning: ConsoleTheme.amber
        case .error: ConsoleTheme.red
        }
    }

    private var background: Color {
        switch tone {
        case .neutral: ConsoleTheme.black
        case .good: Color(red: 0.06, green: 0.25, blue: 0.16)
        case .warning: ConsoleTheme.amberDeep
        case .error: ConsoleTheme.redDeep
        }
    }
}

private func panelBackground(_ color: Color, radius: CGFloat) -> some View {
    RoundedRectangle(cornerRadius: radius)
        .fill(color)
        .overlay(RoundedRectangle(cornerRadius: radius).stroke(ConsoleTheme.line, lineWidth: 1))
}
