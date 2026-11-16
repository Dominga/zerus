package app.zerus.mobile

import app.zerus.mobile.replica.TaskTarget

internal object TaskContentCapabilities {
    fun upload(context: TaskEditContext, target: TaskTarget, connections: List<Connection>, machines: List<Machine>,
               features: Map<MachineKey, Set<String>>): Boolean {
        val connection = connections.singleOrNull { it.id == context.connection.id } ?: return false
        if (connection.url != context.connection.url || connection.token != context.connection.token ||
            connection.protocolVersion != context.connection.protocolVersion || context.key.connectionId != connection.id) return false
        val bound = (target as? TaskTarget.Session)?.session?.machine
        if (bound == null && context.computer.machineId.isEmpty()) return false
        val matches = machines.filter { it.connectionId == connection.id &&
            (if (bound != null) it.machineId == bound else it.id == context.computer.id && it.machineId == context.computer.machineId) }
        val computer = matches.singleOrNull() ?: return false
        return TaskSyncPlan.UPLOAD_FEATURE in features[MachineKey(connection.id, computer.id)].orEmpty()
    }
}
