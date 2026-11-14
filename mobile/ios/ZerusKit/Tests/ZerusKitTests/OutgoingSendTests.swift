import Foundation
import Synchronization
import XCTest
@testable import ZerusKit

/// The production acquisition/settlement boundary, with the real HTTP reader and
/// ordered durable writer. No gateway, native agent or phone is used.
@MainActor
final class OutgoingSendTests: XCTestCase {
    private nonisolated static let original = "11111111-1111-4111-8111-111111111111"
    private nonisolated static let queued = #"{"request_id":"11111111-1111-4111-8111-111111111111","state":"queued"}"#
    private nonisolated static let completed = #"{"request_id":"11111111-1111-4111-8111-111111111111","state":"completed","result":{"status":"submitted"}}"#

    private final class Sink: Sendable {
        let entered = Mutex(false)
        let written = Mutex<[MessageState]>([])
        let delay: TimeInterval
        init(delay: TimeInterval) { self.delay = delay }
        func write(_ value: MessageState) {
            if value.outgoing.first?.status == "submitted" {
                entered.withLock { $0 = true }
                Thread.sleep(forTimeInterval: delay)
            }
            written.withLock { $0.append(value) }
        }
    }

    @MainActor
    private final class Flight {
        let connection = Connection(id: "gateway", label: "Fixture", url: "https://gateway.example", token: "synthetic-token")
        let target = Target(computerId: "computer", session: "session", run: "run", conversation: "conversation", connectionId: "gateway")
        let client = StubProtocol.client()
        let sink: Sink
        let store: OrderedStatePersistence<MessageState>
        var active = true
        var failure: Error?
        var received: JSONObject?

        init(writeDelay: TimeInterval = 0) {
            let target = Target(computerId: "computer", session: "session", run: "run", conversation: "conversation", connectionId: "gateway")
            let initial = MessageState(drafts: [Draft(target: target, text: "Newer editor", revision: "newer")],
                outgoing: [OutgoingMessage(requestId: OutgoingSendTests.original, target: target, text: "Original message", attachments: [], createdAt: 1)])
            let sink = Sink(delay: writeDelay)
            self.sink = sink
            store = OrderedStatePersistence(initial: initial) { sink.write($0) }
        }

        func run(limit: Duration, pollDelay: Duration = .milliseconds(50)) async {
            defer { active = false }
            let client = client, connection = connection, target = target, id = OutgoingSendTests.original
            do {
                let payload = try target.json().putting("request_id", .string(id)).putting("text", "Original message")
                try await OutgoingSend.run(limit: limit,
                    submit: { try await client.submit(connection, target: target, operation: "send", payload: payload, id: id) },
                    awaitReceipt: { try await client.waitForReceipt(connection, initial: $0, pollDelay: pollDelay) },
                    settle: { receipt in
                        self.received = receipt
                        _ = try? await self.store.durable { current in
                            MessageChanges.result(current, id) { original in
                                var result = original; result.status = "submitted"; result.submittedAt = 2
                                return result
                            }
                        }
                    })
            } catch {
                failure = error
                let status = error is OutgoingSend.Rejected ? "failed" : "uncertain"
                _ = try? await store.durable { current in
                    MessageChanges.result(current, id) { original in
                        var result = original; result.status = status
                        return result
                    }
                }
            }
        }
    }

    /// A test ceiling cancels the original task if the production deadline is
    /// missing, so the RED control never leaves its HTTP fixture running.
    private func finish(_ flight: Flight, _ task: Task<Void, Never>, ceiling: Duration = .seconds(1)) async -> Duration {
        let began = ContinuousClock.now
        while flight.active && ContinuousClock.now - began < ceiling { try? await Task.sleep(for: .milliseconds(5)) }
        if flight.active { task.cancel() }
        await task.value
        return ContinuousClock.now - began
    }

    private func originalIsKept(_ flight: Flight, status: String, line: UInt = #line) {
        XCTAssertEqual(flight.store.state.outgoing.count, 1, line: line)
        XCTAssertEqual(flight.store.state.outgoing.first?.requestId, Self.original, line: line)
        XCTAssertEqual(flight.store.state.outgoing.first?.text, "Original message", line: line)
        XCTAssertEqual(flight.store.state.outgoing.first?.target, flight.target, line: line)
        XCTAssertEqual(flight.store.state.outgoing.first?.status, status, line: line)
        XCTAssertEqual(flight.store.state.drafts.first?.text, "Newer editor", line: line)
    }

    private func stopped() async {
        let began = ContinuousClock.now
        while StubProtocol.stoppedHanging.withLock({ $0 }) == 0 && ContinuousClock.now - began < .seconds(1) {
            try? await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(StubProtocol.stoppedHanging.withLock { $0 }, 1)
    }

    func testHungSubmissionExpiresAndKeepsOneOriginalUnknown() async {
        StubProtocol.reset { _ in .hang }
        let flight = Flight(); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .milliseconds(150)) }
        let elapsed = await finish(flight, task)
        XCTAssertEqual(flight.failure as? ZerusError, Deadline.expired)
        XCTAssertLessThan(elapsed, .milliseconds(800))
        originalIsKept(flight, status: "uncertain")
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        XCTAssertEqual(StubProtocol.requests.first?.json?.string("request_id"), Self.original)
        await stopped()
    }

    func testHungOriginalReceiptExpiresWithoutAnotherSubmission() async {
        StubProtocol.reset { request in request.method == "POST" ? StubProtocol.json(200, Self.queued) : .hang }
        let flight = Flight(); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .milliseconds(200)) }
        _ = await finish(flight, task)
        XCTAssertEqual(flight.failure as? ZerusError, Deadline.expired)
        originalIsKept(flight, status: "uncertain")
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST", "GET"])
        XCTAssertEqual(StubProtocol.requests.last?.url.path, "/v1/requests/" + Self.original)
        await stopped()
    }

    func testPartialResponseBodyIsCancelledByTheSameDeadline() async {
        StubProtocol.reset { _ in .partial(status: 200, body: Data("{\"request_id\":".utf8)) }
        let flight = Flight(); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .milliseconds(150)) }
        _ = await finish(flight, task)
        XCTAssertEqual(flight.failure as? ZerusError, Deadline.expired)
        originalIsKept(flight, status: "uncertain")
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        await stopped()
    }

    func testExpiryDuringPollDelayNeverStartsALaterReadOrReplacementPost() async throws {
        StubProtocol.reset { _ in StubProtocol.json(200, Self.queued) }
        let flight = Flight(); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .milliseconds(150), pollDelay: .seconds(1)) }
        _ = await finish(flight, task)
        XCTAssertEqual(flight.failure as? ZerusError, Deadline.expired)
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        originalIsKept(flight, status: "uncertain")
    }

    func testAnAcceptedDurableWriteCrossesTheClockAndKeepsItsFlightUntilCommitted() async throws {
        StubProtocol.reset { _ in StubProtocol.json(200, Self.completed) }
        let flight = Flight(writeDelay: 0.4); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .milliseconds(150)) }
        let began = ContinuousClock.now
        while !flight.sink.entered.withLock({ $0 }) && ContinuousClock.now - began < .seconds(1) {
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertTrue(flight.sink.entered.withLock { $0 })
        try await Task.sleep(for: .milliseconds(220))
        XCTAssertTrue(flight.active, "The original flight must outlive an accepted durable write.")
        _ = await finish(flight, task)
        XCTAssertNil(flight.failure)
        XCTAssertEqual(flight.received?.string("request_id"), Self.original)
        originalIsKept(flight, status: "submitted")
        XCTAssertEqual(flight.sink.written.withLock { $0.map { $0.outgoing.first?.status } }, ["submitted"])
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
    }

    func testOnlySubmissionRejectionsBecomeFailedAndPollRejectionsStayUnknown() async {
        for status in RelayError.rejectionStatuses.sorted() {
            StubProtocol.reset { _ in StubProtocol.json(status, #"{"error":"Refused original"}"#) }
            let flight = Flight(); defer { flight.store.close() }
            await flight.run(limit: .seconds(1))
            XCTAssertEqual((flight.failure as? OutgoingSend.Rejected)?.response.status, status)
            originalIsKept(flight, status: "failed")
            XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        }
        StubProtocol.reset { request in request.method == "POST" ? StubProtocol.json(200, Self.queued) : StubProtocol.json(404, "{}") }
        let flight = Flight(); defer { flight.store.close() }
        await flight.run(limit: .seconds(1))
        XCTAssertEqual((flight.failure as? RelayError)?.status, 404)
        originalIsKept(flight, status: "uncertain")
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST", "GET"])
    }

    func testCallerCancellationKeepsUnknownAndCancelsTheOriginalHttpTask() async throws {
        StubProtocol.reset { _ in .hang }
        let flight = Flight(); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .seconds(45)) }
        let began = ContinuousClock.now
        while StubProtocol.requests.isEmpty && ContinuousClock.now - began < .seconds(1) {
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(StubProtocol.requests.count, 1)
        task.cancel(); _ = await finish(flight, task)
        XCTAssertNotNil(flight.failure)
        XCTAssertNotEqual(flight.failure as? ZerusError, Deadline.expired)
        originalIsKept(flight, status: "uncertain")
        await stopped()
    }

    func testAnAmbiguousSubmissionFailureNeverBecomesARejection() async {
        for status in [500, 503] {
            StubProtocol.reset { _ in StubProtocol.json(status, "{}") }
            let flight = Flight(); defer { flight.store.close() }
            await flight.run(limit: .seconds(1))
            XCTAssertEqual((flight.failure as? RelayError)?.status, status)
            originalIsKept(flight, status: "uncertain")
            XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        }
    }

    func testCallerCancellationAfterAnAcceptedWriteStillRecordsItsReceipt() async throws {
        StubProtocol.reset { _ in StubProtocol.json(200, Self.completed) }
        let flight = Flight(writeDelay: 0.3); defer { flight.store.close() }
        let task = Task { await flight.run(limit: .seconds(1)) }
        let began = ContinuousClock.now
        while !flight.sink.entered.withLock({ $0 }) && ContinuousClock.now - began < .seconds(1) {
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertTrue(flight.sink.entered.withLock { $0 })
        task.cancel(); _ = await finish(flight, task)
        XCTAssertNil(flight.failure)
        originalIsKept(flight, status: "submitted")
        XCTAssertEqual(flight.sink.written.withLock { $0.map { $0.outgoing.first?.status } }, ["submitted"])
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
    }
}
