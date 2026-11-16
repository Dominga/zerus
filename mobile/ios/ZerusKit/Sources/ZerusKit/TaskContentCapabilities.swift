import Foundation
import ZerusReplica

public enum TaskContentCapabilities {
    public static func upload(_ context: TaskContentContext, target: TaskTarget, connections: [Connection], machines: [Machine], features: [MachineKey: Set<String>]) -> Bool {
        let connections = connections.filter { $0.id == context.connection.id }
        guard connections.count == 1, let connection = connections.first,
              connection.url == context.connection.url, connection.token == context.connection.token,
              connection.protocolVersion == context.connection.protocolVersion, context.key.connectionId == connection.id else { return false }
        let bound: String? = if case .session(let session) = target { session.machine } else { nil }
        guard bound != nil || !context.computer.machineId.isEmpty else { return false }
        let matches = machines.filter { $0.connectionId == connection.id && (bound != nil ? $0.machineId == bound : $0.id == context.computer.id && $0.machineId == context.computer.machineId) }
        guard matches.count == 1 else { return false }
        return features[MachineKey(connectionId: connection.id, computerId: matches[0].id), default: []].contains(TaskSyncPlan.uploadFeature)
    }
}
