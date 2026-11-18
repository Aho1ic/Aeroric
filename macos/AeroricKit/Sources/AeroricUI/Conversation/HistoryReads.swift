import Foundation

/// B2-3: which history read may still write back. Each read takes a number;
/// only the newest one's reply (or error) lands, so a slow older page — the
/// session switched, "load earlier" pressed again — never overwrites it.
struct HistoryReads {
    private(set) var newest = 0
    private(set) var running: Task<Void, Never>?

    /// A new read: the one before is cancelled and will not write.
    mutating func begin(_ task: (Int) -> Task<Void, Never>) {
        running?.cancel()
        newest += 1
        running = task(newest)
    }

    func lands(_ read: Int) -> Bool { read == newest }

    /// An error worth showing: not the cancelled older read.
    static func shows(_ error: Error) -> Bool {
        !(error is CancellationError) && !Task.isCancelled
    }
}
