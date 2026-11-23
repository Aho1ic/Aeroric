#if os(iOS) && canImport(VisionKit)
import SwiftUI
import VisionKit

/// D4 step 7 (2026-11-23): scan the pairing QR code on the Mac's screen.
/// QR codes only; the first one read is handed over once and scanning
/// stops. Whatever it reads only fills the pairing page — the user still
/// taps 配对 (see `CompanionConnector.offerPairingLink`).
public struct CompanionPairingScanner: UIViewControllerRepresentable {
    private let onFound: @MainActor (String) -> Void

    public init(onFound: @escaping @MainActor (String) -> Void) {
        self.onFound = onFound
    }

    /// Why the camera cannot scan here, or nil when it can.
    @MainActor
    public static var unavailableReason: String? {
        if !DataScannerViewController.isSupported { return "这台设备不支持扫码，请粘贴链接" }
        if !DataScannerViewController.isAvailable {
            return "相机现在用不了（可能没给相机权限），可以在“设置”里打开，或粘贴链接"
        }
        return nil
    }

    public func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            recognizesMultipleItems: false,
            isHighFrameRateTrackingEnabled: false,
            isHighlightingEnabled: true)
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    public func updateUIViewController(_ scanner: DataScannerViewController, context: Context) {}

    public static func dismantleUIViewController(_ scanner: DataScannerViewController, coordinator: Coordinator) {
        scanner.stopScanning()
    }

    public func makeCoordinator() -> Coordinator { Coordinator(onFound: onFound) }

    @MainActor
    public final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        private let onFound: @MainActor (String) -> Void
        private var handedOver = false

        init(onFound: @escaping @MainActor (String) -> Void) {
            self.onFound = onFound
        }

        public func dataScanner(_ dataScanner: DataScannerViewController, didAdd addedItems: [RecognizedItem],
                                allItems: [RecognizedItem]) {
            guard !handedOver else { return }
            for item in addedItems {
                if case let .barcode(code) = item, let text = code.payloadStringValue, !text.isEmpty {
                    handedOver = true
                    dataScanner.stopScanning()
                    onFound(text)
                    return
                }
            }
        }
    }
}
#endif
