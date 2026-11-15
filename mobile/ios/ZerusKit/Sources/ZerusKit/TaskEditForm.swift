import Foundation
import ZerusReplica

/// Local presentation, independent of current selection. A restored form grants no edit authority.
public struct TaskEditForm: Equatable, Sendable {
    public var title: String, text: String
    public var kept: [TaskMediaInfo]
    public var files: [Attachment] = []
    public var target: TaskTarget?
    public var rank: String?
    public var comment = ""
    public var commentFiles: [Attachment] = []
    public var sequence: Int64 = 0
    public let originalTarget: TaskTarget
    public let originalRank: String
    public init(_ view: TaskEditView) {
        title = view.title; text = view.text; kept = view.attachments; originalTarget = view.target; originalRank = view.rank
    }
    public func changed(_ view: TaskEditView) -> Bool {
        title != view.title || text != view.text || kept.map(\.token) != view.attachments.map(\.token) || !files.isEmpty || target != nil || rank != nil ||
        !comment.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !commentFiles.isEmpty
    }
    private static func targetJSON(_ target: TaskTarget?) -> JSON {
        switch target {
        case nil: .null
        case .anySession?: .object(["kind": "any"])
        case .session(let session)?: .object(["kind": "session", "machine": .string(session.machine), "session": .string(session.session), "session_id": .string(session.sessionId)])
        }
    }
    public func json() throws -> String {
        let media: [JSON] = kept.map { .object(["name": .string($0.name), "mime": .string($0.mime), "reference": .string($0.reference),
                                               "size": .int(Int64($0.size)), "sha256": .string($0.sha256), "token": .string($0.token)]) }
        return String(decoding: try JSON.object(["version": 1, "title": .string(title), "text": .string(text), "kept": .array(media),
            "files": .array(files.map { .object($0.json()) }), "target": Self.targetJSON(target), "rank": rank.map(JSON.string) ?? .null,
            "comment": .string(comment), "comment_files": .array(commentFiles.map { .object($0.json()) }),
            "original_target": Self.targetJSON(originalTarget), "original_rank": .string(originalRank)]).data(), as: UTF8.self)
    }
    public static func read(_ text: String, sequence: Int64) throws -> Self {
        guard let value = try JSON.parse(Data(text.utf8)).object else { throw ZerusError.invalid("The saved task form is unavailable. Its bytes were kept.") }
        try WorkspaceProtocol.fields(value, ["version", "title", "text", "kept", "files", "target", "rank", "comment", "comment_files", "original_target", "original_rank"])
        guard try WorkspaceProtocol.number(value, "version") == 1, sequence >= 0,
              let kept = value["kept"]?.array, let files = value["files"]?.array, let comments = value["comment_files"]?.array,
              kept.count <= 8, files.count + comments.count <= 8 else { throw ZerusError.invalid("The saved task form is unavailable. Its bytes were kept.") }
        func target(_ name: String) throws -> TaskTarget? {
            if value[name]?.isNull == true { return nil }
            guard let raw = value[name]?.object else { throw ZerusError.invalid("The saved task target is unavailable.") }
            switch try WorkspaceProtocol.string(raw, "kind") {
            case "any": try WorkspaceProtocol.fields(raw, ["kind"]); return .anySession
            case "session":
                try WorkspaceProtocol.fields(raw, ["kind", "machine", "session", "session_id"])
                return try .session(session: SessionRef(machine: WorkspaceProtocol.string(raw, "machine"), session: WorkspaceProtocol.string(raw, "session"), sessionId: WorkspaceProtocol.string(raw, "session_id")))
            default: throw ZerusError.invalid("The saved task target is unavailable.")
            }
        }
        func attachments(_ rows: [JSON]) throws -> [Attachment] {
            try rows.map { row in
                guard let raw = row.object else { throw ZerusError.invalid("The saved attachment is unavailable.") }
                try WorkspaceProtocol.fields(raw, ["id", "name", "mime", "bytes", "sha256"])
                let file = try Attachment(id: WorkspaceProtocol.string(raw, "id"), name: WorkspaceProtocol.string(raw, "name"), mime: WorkspaceProtocol.string(raw, "mime"), bytes: WorkspaceProtocol.number(raw, "bytes"), sha256: WorkspaceProtocol.string(raw, "sha256"))
                try AttachmentPolicy.validate(file); return file
            }
        }
        guard let original = try target("original_target") else { throw ZerusError.invalid("The saved task target is unavailable.") }
        let media = try kept.map { row -> TaskMediaInfo in
            guard let raw = row.object else { throw ZerusError.invalid("The saved task media is unavailable.") }
            try WorkspaceProtocol.fields(raw, ["name", "mime", "reference", "size", "sha256", "token"])
            let size = try WorkspaceProtocol.number(raw, "size")
            guard size >= 0 && size <= AttachmentPolicy.maxFileBytes else { throw ZerusError.invalid("The saved task media is unavailable.") }
            return try TaskMediaInfo(name: WorkspaceProtocol.string(raw, "name"), mime: WorkspaceProtocol.string(raw, "mime"), reference: WorkspaceProtocol.string(raw, "reference"), size: UInt64(size), sha256: WorkspaceProtocol.string(raw, "sha256"), token: WorkspaceProtocol.string(raw, "token"))
        }
        var result = try Self(TaskEditView(project: "", task: "", title: WorkspaceProtocol.string(value, "title"), text: WorkspaceProtocol.string(value, "text"), target: original, rank: WorkspaceProtocol.string(value, "original_rank"), attachments: media))
        result.files = try attachments(files); result.commentFiles = try attachments(comments); result.target = try target("target")
        result.rank = value["rank"]?.isNull == true ? nil : try WorkspaceProtocol.string(value, "rank")
        result.comment = try WorkspaceProtocol.string(value, "comment"); result.sequence = sequence
        return result
    }
    /// Reads authenticated private copies off the main actor; no URI or mutable provider is consulted.
    public func input(view: TaskEditView, assets: AttachmentStore) async throws -> TaskEditInput {
        try await taskEditIO {
            func media(_ files: [Attachment]) throws -> [TaskMediaInput] {
                try AttachmentPolicy.selection(files)
                return try files.map { file in TaskMediaInput(name: file.name, mime: file.mime, data: try assets.decrypt(file), reference: "") }
            }
            let changed = title != view.title || text != view.text || kept.map(\.token) != view.attachments.map(\.token) || !files.isEmpty
            let content = changed ? try TaskEditContent(title: title, text: text, kept: kept.map(\.token), files: media(files)) : nil
            return try TaskEditInput(content: content, target: target, rank: rank, comment: comment, commentFiles: media(commentFiles))
        }
    }
}
