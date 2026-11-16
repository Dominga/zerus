import Foundation
import XCTest
@testable import ZerusKit
import ZerusReplica

final class TaskContentCapabilitiesTests: XCTestCase {
    func testOriginalConnectionAndExactTargetHostAdvertisement() {
        let connection = Connection(id: "connection", label: "Original label", url: "https://relay.example.test", token: "private")
        let computer = Machine(connectionId: connection.id, id: "computer", name: "Original label", online: true, machineId: "native-machine")
        let context = TaskContentContext(connection: connection, key: ProjectKey(connectionId: connection.id, swarmId: "swarm", id: "project"), projectName: "Fixture", computer: computer)
        let features: [MachineKey: Set<String>] = [computer.key: [TaskSyncPlan.uploadFeature]]
        func allowed(_ connections: [Connection] = [connection], machines: [Machine] = [computer], advertised: [MachineKey: Set<String>] = features, target: TaskTarget = .anySession) -> Bool {
            TaskContentCapabilities.upload(context, target: target, connections: connections, machines: machines, features: advertised)
        }
        XCTAssertTrue(allowed())
        var renamed = connection; renamed.label = "Renamed"; var renamedHost = computer; renamedHost.name = "Renamed host"
        XCTAssertTrue(allowed([renamed], machines: [renamedHost]))
        XCTAssertFalse(allowed(advertised: [:]))
        XCTAssertFalse(allowed(advertised: [MachineKey(connectionId: "foreign", computerId: computer.id): [TaskSyncPlan.uploadFeature]]))
        XCTAssertFalse(allowed(advertised: [MachineKey(connectionId: connection.id, computerId: "foreign"): [TaskSyncPlan.uploadFeature]]))
        XCTAssertFalse(allowed([Connection(id: connection.id, label: connection.label, url: connection.url, token: "other credential")]))
        var replaced = computer; replaced.machineId = "replacement"; XCTAssertFalse(allowed(machines: [replaced]))
        XCTAssertFalse(allowed(machines: [computer, computer]))
        XCTAssertFalse(allowed(target: .session(session: SessionRef(machine: "foreign", session: "label", sessionId: "launch_id:opaque"))))
        XCTAssertTrue(allowed(target: .session(session: SessionRef(machine: computer.machineId, session: "renamed-label", sessionId: "launch_id:opaque"))))
    }
}
