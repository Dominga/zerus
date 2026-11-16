package app.zerus.mobile

import app.zerus.mobile.replica.*
import org.junit.Assert.*
import org.junit.Test

class TaskContentCapabilitiesTest {
    @Test fun uploadRequiresOriginalConnectionAndExactTargetHostAdvertisement() {
        val connection = Connection("connection", "Original label", "https://relay.example.test", "private")
        val computer = Machine(connection.id, "computer", "Original host label", true, machineId = "native-machine")
        val context = TaskEditContext(connection, ProjectKey(connection.id, "swarm", "project"), "Fixture", computer, emptyMap())
        val features = mapOf(MachineKey(connection.id, computer.id) to setOf(TaskSyncPlan.UPLOAD_FEATURE))
        fun allowed(connections: List<Connection> = listOf(connection), machines: List<Machine> = listOf(computer), advertised: Map<MachineKey, Set<String>> = features,
                    target: TaskTarget = TaskTarget.AnySession) = TaskContentCapabilities.upload(context, target, connections, machines, advertised)
        assertTrue(allowed())
        assertTrue(allowed(connections = listOf(connection.copy(label = "Renamed")), machines = listOf(computer.copy(name = "Renamed host"))))
        assertFalse(allowed(advertised = emptyMap()))
        assertFalse(allowed(advertised = mapOf(MachineKey("foreign", computer.id) to setOf(TaskSyncPlan.UPLOAD_FEATURE))))
        assertFalse(allowed(advertised = mapOf(MachineKey(connection.id, "foreign") to setOf(TaskSyncPlan.UPLOAD_FEATURE))))
        assertFalse(allowed(connections = listOf(connection.copy(token = "other credential"))))
        assertFalse(allowed(machines = listOf(computer.copy(machineId = "replacement"))))
        assertFalse(allowed(machines = listOf(computer, computer)))
        assertFalse(allowed(target = TaskTarget.Session(SessionRef("foreign-machine", "old-label", "launch_id:opaque"))))
        assertTrue(allowed(target = TaskTarget.Session(SessionRef(computer.machineId, "renamed-label", "launch_id:opaque"))))
    }
}
