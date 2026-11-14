import CryptoKit
import Foundation
import Synchronization
import XCTest
@testable import ZerusKit

@MainActor
final class SendCampaignTests: XCTestCase {
    private nonisolated static let id = "22222222-2222-4222-8222-222222222222"
    private nonisolated static let queued = #"{"request_id":"22222222-2222-4222-8222-222222222222","state":"queued"}"#
    private nonisolated static let completed = #"{"request_id":"22222222-2222-4222-8222-222222222222","state":"completed","result":{"status":"submitted"}}"#
    private nonisolated static let answers = #"[{"question_id":"question","text":"Original answer"}]"#

    private final class Sink: Sendable {
        let entered = Mutex(false)
        let written = Mutex<[MessageState]>([])
        let delay: TimeInterval
        init(_ delay: TimeInterval) { self.delay = delay }
        func write(_ state: MessageState) {
            if state.drafts.contains(where: { $0.status == "submitted" }) {
                entered.withLock { $0 = true }
                Thread.sleep(forTimeInterval: delay)
            }
            written.withLock { $0.append(state) }
        }
    }

    /// Owns only a private sealed fixture. The bounded fallback releases its
    /// lock even against an unbounded/cancellation-blind preparation mutant.
    private final class HeldStore: Sendable {
        let entered = Mutex(false)
        let released = Mutex(false)
        let release = DispatchSemaphore(value: 0)
        init(_ files: AttachmentStore) {
            DispatchQueue.global().async {
                files.lock.lock()
                self.entered.withLock { $0 = true }
                _ = self.release.wait(timeout: .now() + 1.4)
                files.lock.unlock()
                self.released.withLock { $0 = true }
            }
        }
    }

    @MainActor
    private final class Campaign {
        let connection = Connection(id: "gateway", label: "Fixture", url: "https://gateway.example", token: "synthetic-token")
        let target = Target(computerId: "computer", session: "session", run: "run", conversation: "conversation", connectionId: "gateway")
        let client = StubProtocol.client()
        let files: AttachmentStore?
        let original: Draft
        let attachments: [Attachment]
        let sink: Sink
        let state: OrderedStatePersistence<MessageState>
        var active = true
        var attempted = false
        var failure: Error?

        init(files: AttachmentStore? = nil, attachments: [Attachment] = [], writeDelay: TimeInterval = 0) {
            self.files = files
            self.attachments = attachments
            original = Draft(target: target, text: "", status: "submitting", requestId: SendCampaignTests.id,
                             questionId: "question", questionHash: "hash", answers: SendCampaignTests.answers)
            sink = Sink(writeDelay)
            let sink = sink
            let initial = files == nil
                ? MessageState(drafts: [original, Draft(target: target, text: "Newer editor")])
                : MessageState(drafts: [Draft(target: target, text: "Newer editor")], outgoing: [
                    OutgoingMessage(requestId: SendCampaignTests.id, target: target, text: "Original attachment message", attachments: attachments, createdAt: 1)])
            state = OrderedStatePersistence(initial: initial) { sink.write($0) }
        }

        var status: String? { files == nil ? state.state.drafts.first?.status : state.state.outgoing.first?.status }

        func run(limit: Duration = .milliseconds(150), preparationLimit: Duration = .milliseconds(150)) async {
            defer { active = false }
            let original = original, id = SendCampaignTests.id, answer = files == nil
            do {
                var payload = try target.json().putting("request_id", .string(id))
                if let files {
                    payload["text"] = "Original attachment message"
                    payload["attachments"] = try await files.preparedPayload(attachments, limit: preparationLimit)
                } else {
                    payload["question_id"] = .string(original.questionId)
                    payload["expected_question_hash"] = .string(original.questionHash)
                    payload["answers"] = try JSON.parse(original.answers)
                }
                try Task.checkCancellation()
                attempted = true
                try await OutgoingSend.run(client: client, connection: connection, target: target,
                    operation: files == nil ? "answer" : "send", payload: payload, id: id, limit: limit, settle: { receipt in
                        _ = try? await self.state.durable { state in
                            if answer { return QuestionTracking.settle(state, original, receipt) }
                            return MessageChanges.result(state, id) { var copy = $0; copy.status = "submitted"; return copy }
                        }
                    })
            } catch {
                failure = error
                let status = !attempted || error is OutgoingSend.Rejected ? "failed" : "uncertain"
                _ = try? await state.durable { state in
                    var copy = state
                    copy.drafts = state.drafts.map { draft in
                        guard draft.key == original.key, draft.requestId == original.requestId,
                              draft.generation == original.generation, !QuestionTracking.locked(draft) else { return draft }
                        var result = draft; result.status = status; return result
                    }
                    return MessageChanges.result(copy, id) { var result = $0; result.status = status; return result }
                }
            }
        }
    }

    private func finish(_ campaign: Campaign, _ task: Task<Void, Never>) async -> Duration {
        let start = ContinuousClock.now
        while campaign.active && ContinuousClock.now - start < .seconds(2) { try? await Task.sleep(for: .milliseconds(5)) }
        if campaign.active { task.cancel() }
        await task.value
        return ContinuousClock.now - start
    }

    private func kept(_ campaign: Campaign, _ status: String, line: UInt = #line) {
        XCTAssertEqual(campaign.status, status, line: line)
        XCTAssertEqual(campaign.state.state.drafts.last?.text, "Newer editor", line: line)
        if campaign.files == nil {
            XCTAssertEqual(campaign.state.state.drafts.first?.requestId, Self.id, line: line)
            XCTAssertEqual(campaign.state.state.drafts.first?.answers, Self.answers, line: line)
            XCTAssertEqual(campaign.state.state.drafts.first?.generation, campaign.original.generation, line: line)
        } else {
            XCTAssertEqual(campaign.state.state.outgoing.count, 1, line: line)
            XCTAssertEqual(campaign.state.state.outgoing.first?.requestId, Self.id, line: line)
            XCTAssertEqual(campaign.state.state.outgoing.first?.attachments, campaign.attachments, line: line)
        }
    }

    private func stopped(_ count: Int = 1) async {
        let start = ContinuousClock.now
        while StubProtocol.stoppedHanging.withLock({ $0 }) < count && ContinuousClock.now - start < .seconds(1) {
            try? await Task.sleep(for: .milliseconds(5))
        }
        XCTAssertEqual(StubProtocol.stoppedHanging.withLock { $0 }, count)
    }

    func testAnswerSubmissionHasOneDeadlineAndKeepsItsFrozenQuestionAndAnswers() async {
        StubProtocol.reset { _ in .hang }
        let campaign = Campaign(); defer { campaign.state.close() }
        let elapsed = await finish(campaign, Task { await campaign.run() })
        XCTAssertLessThan(elapsed, .seconds(1))
        XCTAssertEqual(campaign.failure as? ZerusError, Deadline.expired)
        kept(campaign, "uncertain")
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        XCTAssertEqual(StubProtocol.requests.first?.json?.string("operation"), "answer")
        let payload = StubProtocol.requests.first?.json?.object("payload")
        XCTAssertEqual(payload?.string("request_id"), Self.id)
        XCTAssertEqual(payload?.string("expected_question_hash"), "hash")
        XCTAssertEqual(payload?["answers"], try? JSON.parse(Self.answers))
        await stopped()
    }

    func testAnswerOriginalReceiptAndDribblingBodyShareTheAcquisitionClock() async {
        for partial in [false, true] {
            StubProtocol.reset { request in
                if request.method == "POST" { return StubProtocol.json(200, Self.queued) }
                return partial ? .partial(status: 200, body: Data("{\"state\":".utf8)) : .hang
            }
            let campaign = Campaign(); defer { campaign.state.close() }
            _ = await finish(campaign, Task { await campaign.run(limit: .milliseconds(1100)) })
            XCTAssertEqual(campaign.failure as? ZerusError, Deadline.expired)
            kept(campaign, "uncertain")
            XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST", "GET"])
            XCTAssertEqual(StubProtocol.requests.last?.url.path, "/v1/requests/" + Self.id)
            await stopped()
        }
    }

    func testAnswerOnlySubmissionRefusalMeansNotSent() async {
        for reading in [false, true] {
            StubProtocol.reset { request in
                reading && request.method == "POST" ? StubProtocol.json(200, Self.queued) : StubProtocol.json(403, "{}")
            }
            let campaign = Campaign(); defer { campaign.state.close() }
            await campaign.run(limit: .seconds(2))
            kept(campaign, reading ? "uncertain" : "failed")
            XCTAssertEqual(campaign.failure is OutgoingSend.Rejected, !reading)
            XCTAssertEqual(StubProtocol.requests.map(\.method), reading ? ["POST", "GET"] : ["POST"])
        }
    }

    func testAnswerAndAttachmentCancellationAfterSubmissionKeepTheOriginalUnknown() async throws {
        let (root, files, attachment) = try fixture(); defer { try? FileManager.default.removeItem(at: root) }
        for withFile in [false, true] {
            StubProtocol.reset { _ in .hang }
            let campaign = Campaign(files: withFile ? files : nil, attachments: withFile ? [attachment] : [])
            defer { campaign.state.close() }
            let task = Task { await campaign.run(limit: .seconds(45)) }
            let start = ContinuousClock.now
            while StubProtocol.requests.isEmpty && ContinuousClock.now - start < .seconds(1) {
                try await Task.sleep(for: .milliseconds(5))
            }
            XCTAssertEqual(StubProtocol.requests.count, 1)
            task.cancel()
            _ = await finish(campaign, task)
            XCTAssertNotNil(campaign.failure)
            XCTAssertNotEqual(campaign.failure as? ZerusError, Deadline.expired)
            kept(campaign, "uncertain")
            XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
            await stopped()
        }
    }

    func testAnswerDurableAcceptanceOutlivesDeadlineAndCallerCancellation() async throws {
        for cancel in [false, true] {
            StubProtocol.reset { _ in StubProtocol.json(200, Self.completed) }
            let campaign = Campaign(writeDelay: 0.4); defer { campaign.state.close() }
            let task = Task { await campaign.run() }
            let start = ContinuousClock.now
            while !campaign.sink.entered.withLock({ $0 }) && ContinuousClock.now - start < .seconds(1) {
                try await Task.sleep(for: .milliseconds(5))
            }
            XCTAssertTrue(campaign.sink.entered.withLock { $0 })
            if cancel { task.cancel() }
            try await Task.sleep(for: .milliseconds(220))
            XCTAssertTrue(campaign.active)
            _ = await finish(campaign, task)
            XCTAssertNil(campaign.failure)
            kept(campaign, "submitted")
            XCTAssertEqual(campaign.sink.written.withLock { $0.map { $0.drafts.first?.status } }, ["submitted"])
        }
    }

    func testRepeatedAnswerReceiptChecksAreBoundedReadsOfTheSameOriginal() async {
        StubProtocol.reset { _ in .hang }
        let campaign = Campaign(); defer { campaign.state.close() }
        let original = campaign.original
        _ = try? await campaign.state.durable { state in
            var copy = state
            copy.drafts[0].status = "uncertain"
            return copy
        }
        for _ in 0..<2 {
            let client = campaign.client, connection = campaign.connection
            let check = Task {
                do {
                    let receipt = try await client.boundedReceipt(connection, id: original.requestId, limit: .milliseconds(150))
                    _ = try? await campaign.state.durable { QuestionTracking.settle($0, original, receipt) }
                    return false
                } catch { return (error as? ZerusError) == Deadline.expired }
            }
            let ceiling = Task { try? await Task.sleep(for: .seconds(1)); check.cancel() }
            let expired = await check.value
            ceiling.cancel()
            XCTAssertTrue(expired)
            kept(campaign, "uncertain")
        }
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["GET", "GET"])
        XCTAssertTrue(StubProtocol.requests.allSatisfy { $0.url.path == "/v1/requests/" + Self.id })
        await stopped(2)
    }

    private func fixture() throws -> (URL, AttachmentStore, Attachment) {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("send-campaign-\(UUID())")
        let files = AttachmentStore(root: root, keys: StaticKeyProvider(SymmetricKey(size: .bits256)))
        let attachment = try files.importData(Data("Original file bytes".utf8), name: "fixture.txt", mime: "text/plain", current: [])
        return (root, files, attachment)
    }

    func testAttachmentPreparationDeadlineIsLocalOnlyAndKeepsTheSealedOriginal() async throws {
        let (root, files, attachment) = try fixture(); defer { try? FileManager.default.removeItem(at: root) }
        let held = HeldStore(files); defer { held.release.signal() }
        while !held.entered.withLock({ $0 }) { try await Task.sleep(for: .milliseconds(5)) }
        StubProtocol.reset { _ in .hang }
        let campaign = Campaign(files: files, attachments: [attachment]); defer { campaign.state.close() }
        let elapsed = await finish(campaign, Task { await campaign.run() })
        XCTAssertLessThan(elapsed, .milliseconds(900))
        XCTAssertEqual((campaign.failure as? ZerusError)?.message,
                       "Attachments could not be prepared in time. The saved message was not sent. Try again.")
        XCTAssertFalse(campaign.attempted)
        XCTAssertTrue(StubProtocol.requests.isEmpty)
        kept(campaign, "failed")
        XCTAssertTrue(FileManager.default.fileExists(atPath: root.appendingPathComponent(attachment.id + ".bin").path))
    }

    func testAttachmentPreparationCancellationReachesTheWorkerBeforeTheLockIsReleased() async throws {
        let (root, files, attachment) = try fixture(); defer { try? FileManager.default.removeItem(at: root) }
        let held = HeldStore(files); defer { held.release.signal() }
        while !held.entered.withLock({ $0 }) { try await Task.sleep(for: .milliseconds(5)) }
        StubProtocol.reset { _ in .hang }
        let campaign = Campaign(files: files, attachments: [attachment]); defer { campaign.state.close() }
        let task = Task { await campaign.run(preparationLimit: .seconds(45)) }
        try await Task.sleep(for: .milliseconds(50))
        task.cancel()
        let elapsed = await finish(campaign, task)
        XCTAssertLessThan(elapsed, .milliseconds(900))
        XCTAssertTrue(campaign.failure is CancellationError)
        XCTAssertFalse(held.released.withLock { $0 })
        XCTAssertFalse(campaign.attempted)
        XCTAssertTrue(StubProtocol.requests.isEmpty)
        kept(campaign, "failed")
    }

    func testPreparedAttachmentHasItsOwnTransportClockAndNeverPostsAgain() async throws {
        let (root, files, attachment) = try fixture(); defer { try? FileManager.default.removeItem(at: root) }
        StubProtocol.reset { _ in .hang }
        let campaign = Campaign(files: files, attachments: [attachment]); defer { campaign.state.close() }
        _ = await finish(campaign, Task { await campaign.run() })
        XCTAssertEqual(campaign.failure as? ZerusError, Deadline.expired)
        kept(campaign, "uncertain")
        XCTAssertEqual(StubProtocol.requests.map(\.method), ["POST"])
        let encoded = StubProtocol.requests.first?.json?.object("payload")?["attachments"]?.array?.first?.object
        XCTAssertEqual(encoded?.string("data_base64"), Data("Original file bytes".utf8).base64EncodedString())
        XCTAssertEqual(encoded?.string("name"), attachment.name)
        await stopped()
    }
}
