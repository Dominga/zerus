import Foundation

/// One send or answer's receipt acquisition, followed by its durable settlement.
/// The caller keeps its original request and delivery flight until this returns.
public enum OutgoingSend {
    /// Uses one captured mutation request; settlement belongs to its original owner.
    @MainActor
    public static func run(client: RelayClient, connection: Connection, target: Target, operation: String,
                           payload: JSONObject, id: String, limit: Duration = .seconds(45),
                           settle: @escaping @MainActor @Sendable (JSONObject) async -> Void) async throws {
        try await run(limit: limit,
            submit: { try await client.submit(connection, target: target, operation: operation, payload: payload, id: id) },
            awaitReceipt: { try await client.waitForReceipt(connection, initial: $0) }, settle: settle)
    }

    /// Only a refusal during submission proves this attempt was not accepted.
    /// An error while reading its later receipt never creates this proof.
    public struct Rejected: Error, Sendable {
        public let response: RelayError
    }

    @MainActor
    public static func run(limit: Duration = .seconds(45),
                           submit: @escaping @Sendable () async throws -> JSONObject,
                           awaitReceipt: @escaping @Sendable (JSONObject) async throws -> JSONObject,
                           settle: @escaping @MainActor @Sendable (JSONObject) async -> Void) async throws {
        guard limit > .zero, limit <= .seconds(45) else { throw ZerusError.invalid("Invalid send deadline.") }
        let receipt = try await Deadline.within(limit) {
            let initial: JSONObject
            do { initial = try await submit() }
            catch let response as RelayError where response.rejected { throw Rejected(response: response) }
            return try await awaitReceipt(initial)
        }
        // An acknowledged durable write may finish after the network deadline.
        // Do not cancel it or overwrite its accepted result with Unknown.
        await settle(receipt)
    }
}
