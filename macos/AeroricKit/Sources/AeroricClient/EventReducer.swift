import Foundation
import AeroricProtocol

/// One replayed event row (`events.since` result item).
public struct ReplayEvent: Equatable, Sendable {
    public var seq: Quantity
    public var event: String
    public var data: JSONValue

    public init(seq: Quantity, event: String, data: JSONValue) {
        self.seq = seq
        self.event = event
        self.data = data
    }
}

/// A decoded `events.since` batch (`since_response`: `events`/`latestSeq`/`reset`).
public struct ReplayBatch: Equatable, Sendable {
    public var events: [ReplayEvent]
    public var latestSeq: Quantity
    public var reset: Bool

    public init(events: [ReplayEvent], latestSeq: Quantity, reset: Bool) {
        self.events = events
        self.latestSeq = latestSeq
        self.reset = reset
    }

    /// Decodes a `SuccessFrame.result` payload. `nil` when the shape is not a
    /// replay batch — the caller surfaces that as an application error rather
    /// than the reducer guessing.
    public static func decode(_ result: JSONValue) -> ReplayBatch? {
        guard let object = result.objectValue,
              let latest = object["latestSeq"]?.numberText.flatMap(Quantity.init) else { return nil }
        var events: [ReplayEvent] = []
        for row in object["events"]?.arrayValue ?? [] {
            guard let fields = row.objectValue,
                  let seq = fields["seq"]?.numberText.flatMap(Quantity.init),
                  let event = fields["event"]?.stringValue else { return nil }
            events.append(ReplayEvent(seq: seq, event: event, data: fields["data"] ?? .null))
        }
        let reset = object["reset"]?.boolValue ?? false
        return ReplayBatch(events: events, latestSeq: latest, reset: reset)
    }
}

/// What the reducer did with an input. The caller decides what to do with
/// effects — the reducer itself never blocks, retries or crashes: every
/// protocol anomaly (duplicate, gap, epoch change, unknown event name, host
/// trying to call us) is represented as data.
public enum ReducerEffect: Equatable, Sendable {
    /// An event in order; seq advanced to it.
    case applied(EventFrame)
    /// seq ≤ watermark — already applied (reconnect duplicates, replay overlap).
    case duplicateDropped(seq: Quantity)
    /// The stream jumped ahead of the watermark. The frame is NOT applied; the
    /// caller repairs via `events.since { after: watermark }` and replays.
    case replayGap(expected: Quantity, received: Quantity)
    /// Host generation changed (new epoch). Watermark resets to 0; the caller
    /// must re-request a snapshot before trusting incremental events again.
    case epochChanged(from: String, to: String)
    /// The replay ring wrapped past the requested hole (`reset: true`) — the
    /// caller must full-refresh instead of applying the batch incrementally.
    case needsFullRefresh(reason: String)
}

/// Event sequencing state. Mirror of the host's `SnapshotDecision`
/// (`core/crates/aeroric-core/src/events/snapshot.rs`): watermark = last
/// applied seq (0 = fresh connection, no watermark), duplicates dropped,
/// gaps surface, epoch change resets everything.
///
/// Unknown event names are forwarded as `.applied` — they are still events
/// from a peer we authenticated; dropping them here would hide features a
/// newer host added. Typed interpretation happens above this layer.
public struct EventReducer: Sendable {
    /// Last applied seq; 0 = fresh, no watermark yet.
    public private(set) var watermark: Quantity
    public private(set) var epoch: String?

    public init() {
        watermark = Quantity(0)
        epoch = nil
    }

    /// Applies one live event frame. Returns the effects in order.
    public mutating func apply(_ frame: EventFrame) -> [ReducerEffect] {
        var effects: [ReducerEffect] = []
        if frame.epoch != epoch {
            effects.append(.epochChanged(from: epoch ?? "", to: frame.epoch))
            epoch = frame.epoch
            watermark = Quantity(0)
        }
        let seq = frame.seq.uint64Value
        let applied = watermark.uint64Value
        if seq == 0 {
            // seq starts at 1 per the event log; a 0 on the wire is malformed
            // ordering, not an event. Surface it as a gap rather than applying.
            effects.append(.replayGap(expected: Quantity(applied + 1), received: frame.seq))
        } else if seq <= applied {
            effects.append(.duplicateDropped(seq: frame.seq))
        } else if seq == applied + 1 {
            watermark = frame.seq
            effects.append(.applied(frame))
        } else {
            effects.append(.replayGap(expected: Quantity(applied + 1), received: frame.seq))
        }
        return effects
    }

    /// Applies a replay batch in order (host `apply_backfill` mirror). The
    /// caller passes the epoch the batch belongs to (from the connection that
    /// fetched it). `reset` lifts the watermark to `latestSeq` and demands a
    /// full refresh — the hole is unrecoverable by replay.
    public mutating func apply(_ batch: ReplayBatch, epoch: String) -> [ReducerEffect] {
        var effects: [ReducerEffect] = []
        if self.epoch != epoch {
            effects.append(.epochChanged(from: self.epoch ?? "", to: epoch))
            self.epoch = epoch
            watermark = Quantity(0)
        }
        for row in batch.events {
            let seq = row.seq.uint64Value
            let applied = watermark.uint64Value
            if seq == 0 {
                effects.append(.replayGap(expected: Quantity(applied + 1), received: row.seq))
                continue
            }
            if seq <= applied {
                effects.append(.duplicateDropped(seq: row.seq))
                continue
            }
            let frame = EventFrame(event: row.event, epoch: epoch, seq: row.seq, data: row.data, additional: [:])
            watermark = row.seq
            effects.append(.applied(frame))
        }
        if batch.reset {
            watermark = batch.latestSeq
            effects.append(.needsFullRefresh(reason: "replay ring wrapped past the requested hole"))
        }
        return effects
    }
}
