import Foundation

/// A row's parent session as the child's computer lists it (`parent` in `hgs ls --json`).
public struct ParentLink: Hashable, Sendable {
    public let machine: String
    public let sessionId: String
    public let conversationId: String
    public let name: String
    public let state: String
    /// The child's computer verified this parent running there.
    public var live: Bool { state == "live" }
    /// The parent's short name, as the supervised mark and the from-parent line show it.
    public var shortName: String {
        let last = name.split(separator: "/", omittingEmptySubsequences: false).last.map(String.init) ?? ""
        return last.isEmpty ? name : last
    }
}

/// A parent's descendants in its project group: all, working, quiet requests they hold for it
/// (`asking`), requests only the person can answer (`needsYou`) and errors. Informational: it
/// never raises the parent's own attention (spec 2.3).
public struct SessionTeam: Hashable, Sendable {
    public var total = 0, working = 0, asking = 0, needsYou = 0, errors = 0

    public init(total: Int = 0, working: Int = 0, asking: Int = 0, needsYou: Int = 0, errors: Int = 0) {
        self.total = total
        self.working = working
        self.asking = asking
        self.needsYou = needsYou
        self.errors = errors
    }

    /// With the parent's shown native subagents: one counter for both (zerus-sa9qn.16). Their
    /// requests stay the parent's.
    public func withSubagents(_ rows: [SessionCardChild]) -> SessionTeam {
        var team = self
        team.total += rows.count
        team.working += rows.filter { $0.kind == .working }.count
        return team
    }

    /// The toggle's spoken summary, without dots between values.
    public var summary: [String] {
        ["\(working) of \(total) working"] + (needsYou > 0 ? ["Needs you: \(needsYou)"] : []) +
            (asking > 0 ? ["Asking: \(asking)"] : []) + (errors > 0 ? ["Errors: \(errors)"] : [])
    }
}

/// One card in a project group: nested under `parentKey` at `depth`, a parent's `team`, or
/// `fromParent` for a child shown at top level.
public struct SessionCardEntry: Hashable, Sendable, Identifiable {
    public let session: Session
    public var depth = 0
    public var parentKey: String?
    public var lastChild = false
    public var team: SessionTeam?
    public var fromParent: ParentLink?
    public var fromParentKey: String?
    public var id: String { session.target.key }

    public init(session: Session, depth: Int = 0, parentKey: String? = nil, lastChild: Bool = false, team: SessionTeam? = nil,
                fromParent: ParentLink? = nil, fromParentKey: String? = nil) {
        self.session = session
        self.depth = depth
        self.parentKey = parentKey
        self.lastChild = lastChild
        self.team = team
        self.fromParent = fromParent
        self.fromParentKey = fromParentKey
    }
}

/// An account mark: the catalog label, its letter and the account's `0xRRGGBB` color, the
/// label's default palette slot unless its computer keeps one (spec 1.6, zerus-sa9qn.6).
public struct AccountMark: Hashable, Sendable {
    public let label: String
    public let letter: String
    public let color: UInt32
    public let ink: UInt32
    public let provider: String
}

/// Parents whose subagents and child sessions are hidden: one toggle for both, kept across
/// relaunches by machine UUID and session ID, never by name or run. Expanded is the default. A
/// choice made here wins by row key; a row without durable IDs, or one chosen before the stored
/// list loads, keeps its choice while the app runs, newest last and bounded.
public struct TeamCollapse: Hashable, Sendable {
    public static let maxOverrides = 256
    public private(set) var stored: [String]
    /// Choices by row key, oldest first.
    public private(set) var overrides: [String]
    private var collapsedKeys: [String: Bool]
    private var ids: Set<String>

    public init(stored: [String] = [], overrides: [(String, Bool)] = []) {
        self.stored = stored
        self.ids = Set(stored)
        self.overrides = []
        self.collapsedKeys = [:]
        for (key, collapsed) in overrides { record(key, collapsed) }
    }

    private mutating func record(_ key: String, _ collapsed: Bool) {
        overrides.removeAll { $0 == key }
        overrides.append(key)
        collapsedKeys[key] = collapsed
        while overrides.count > Self.maxOverrides { collapsedKeys[overrides.removeFirst()] = nil }
    }

    public func override(_ key: String) -> Bool? { collapsedKeys[key] }

    public func collapsed(_ session: Session, machine: String?) -> Bool {
        if let chosen = collapsedKeys[session.target.key] { return chosen }
        return ChildSessions.teamId(machine, session).map(ids.contains) ?? false
    }

    /// After one choice for row `key`: a choice saved in `saved` (the stored list it produced)
    /// needs no override; otherwise the row keeps it here.
    public func choose(_ key: String, collapsed: Bool, saved: [String]?) -> TeamCollapse {
        var next = self
        if let saved {
            next.stored = saved
            next.ids = Set(saved)
            next.overrides.removeAll { $0 == key }
            next.collapsedKeys[key] = nil
        } else {
            next.record(key, collapsed)
        }
        return next
    }

    /// The same choices over a newly loaded stored list.
    public func loaded(_ stored: [String]) -> TeamCollapse {
        var next = self
        next.stored = stored
        next.ids = Set(stored)
        return next
    }
}

/// Child sessions on iOS (zerus-sa9qn.5), the contract the desktop and Android share
/// (docs/superpowers/specs/2026-11-07-account-labels-child-sessions-design.md): a row's `parent`
/// link nests it under its parent, and a child of a live parent keeps quiet unless only the person
/// can answer it. Pure functions of the `hgs ls --json` rows; the shared fixture
/// client/core/tests/fixtures/supervised-attention.json pins them as the desktop core projects
/// them. Ported from Android `ChildSessions`.
public enum ChildSessions {
    /// Nesting depth bound, as the desktop list.
    public static let maxDepth = 8
    /// Collapsed parents this phone keeps, newest last.
    public static let maxCollapsedTeams = 2048

    static func uuid(_ text: String) -> Bool { MachineCatalog.canonicalUUID(.string(text)) != nil }

    /// A link field as the core reads it: a string of at most 512 bytes without control characters.
    static func field(_ link: JSONObject, _ key: String) -> String {
        guard case .string(let text)? = link[key], text.utf8.count <= 512,
              !text.unicodeScalars.contains(where: { $0.properties.generalCategory == .control }) else { return "" }
        return text
    }

    /// The row's parent link when well formed: a canonical machine UUID, a session ID or
    /// conversation, a name.
    public static func parent(_ raw: JSONObject) -> ParentLink? {
        guard let link = raw.object("parent") else { return nil }
        let parent = ParentLink(machine: field(link, "machine"), sessionId: field(link, "session_id"),
                                conversationId: field(link, "conversation_id"), name: field(link, "name"), state: field(link, "state"))
        let identity = uuid(parent.sessionId) || (parent.sessionId.isEmpty && !parent.conversationId.isEmpty)
        return uuid(parent.machine) && identity && !parent.name.isEmpty ? parent : nil
    }

    /// The parent runs on the child's computer: the supervised mark, muted or not (spec 3.4).
    public static func supervised(_ raw: JSONObject) -> Bool { parent(raw)?.live == true }

    /// A request only the person may answer: an approval (also a native subagent's), signing in
    /// or an authentication failure.
    public static func personOnlyRequest(_ raw: JSONObject) -> Bool {
        func approval(_ child: JSONObject?) -> Bool {
            guard let child else { return false }
            return child["display_state"] == .string("approval") || child["state"] == .string("approval")
        }
        let subagents = raw.object("subagents")
        return raw["phase"] == .string("approval") ||
            raw.object("provider_error")?["error_kind"] == .string("authentication") ||
            raw["activity_summary"] == .string("Sign in required") ||
            (subagents?.keys.contains { approval(subagents?[$0]?.object) } ?? false) ||
            (raw.array("subagent_previews") ?? []).contains { approval($0.object) }
    }

    /// Whether the row's attention is its parent's: a live parent, an explicit host
    /// classification that no request is the person's, and none only the person can answer. An
    /// older or unknown host keeps the person's attention.
    public static func quiet(_ raw: JSONObject) -> Bool {
        supervised(raw) && raw["human_request"] == .bool(false) && !personOnlyRequest(raw)
    }

    /// Gray badges and no notification: muted on all devices, or quiet (spec 2.1, the mute
    /// treatment).
    public static func silenced(_ session: Session) -> Bool {
        SessionNotificationPreferences.muted(session) || quiet(session.raw)
    }

    /// Each listed row by its computer's machine UUID and durable session ID, and by conversation
    /// for a parent an older hgs listed without one. Names never match. Returns, for a row, its
    /// listed parent's key or nil.
    public static func parentIndex(_ rows: [Session], machine: (Session) -> String?) -> @Sendable (Session) -> String? {
        var bySession: [String: String] = [:]
        var byConversation: [String: (key: String, identified: Bool)] = [:]
        for row in rows {
            guard let id = machine(row), !id.isEmpty else { continue }
            let session = row.raw.string("session_id"), conversation = row.raw.string("conversation_id")
            if !session.isEmpty { bySession["\(id)\n\(session)"] = row.target.key }
            if !conversation.isEmpty { byConversation["\(id)\n\(conversation)"] = (row.target.key, !session.isEmpty) }
        }
        let sessions = bySession, conversations = byConversation
        return { row in
            guard let p = parent(row.raw) else { return nil }
            if !p.sessionId.isEmpty, let found = sessions["\(p.machine)\n\(p.sessionId)"] { return found }
            guard !p.conversationId.isEmpty, let old = conversations["\(p.machine)\n\(p.conversationId)"] else { return nil }
            // An identified link only falls back to a parent row that lists no session ID.
            return p.sessionId.isEmpty || !old.identified ? old.key : nil
        }
    }

    /// One project group's visible cards in their order, child sessions nested under a parent
    /// shown in the same list (desktop group-rows.ts). Nesting never hides a row by itself: a loop
    /// or a parent elsewhere leaves the card at top level. A collapsed parent hides its
    /// descendants.
    public static func nest(_ list: [Session], parentKey: (Session) -> String?, online: (Session) -> Bool,
                            collapsed: (Session) -> Bool) -> [SessionCardEntry] {
        let shown = Set(list.map(\.target.key))
        var parentOf: [String: String] = [:]
        for s in list {
            if let p = parentKey(s), p != s.target.key, shown.contains(p) { parentOf[s.target.key] = p }
        }
        if parentOf.isEmpty {
            return list.map { s in
                let link = parent(s.raw)
                return SessionCardEntry(session: s, fromParent: link, fromParentKey: link == nil ? nil : parentKey(s))
            }
        }
        var children: [String: [Session]] = [:]
        for s in list { if let p = parentOf[s.target.key] { children[p, default: []].append(s) } }
        struct Node { let s: Session; let depth: Int; let parent: String?; let last: Bool }
        var nodes: [Node] = []
        var seen = Set<String>()
        func walk(_ root: Session) {
            var stack = [Node(s: root, depth: 0, parent: nil, last: true)]
            while let node = stack.popLast() {
                guard seen.insert(node.s.target.key).inserted else { continue }
                nodes.append(node)
                if node.depth >= maxDepth { continue }
                let kids = (children[node.s.target.key] ?? []).filter { !seen.contains($0.target.key) }
                for i in kids.indices.reversed() {
                    stack.append(Node(s: kids[i], depth: node.depth + 1, parent: node.s.target.key, last: i == kids.count - 1))
                }
            }
        }
        // Cards below a session without a shown parent; the rest sit in a loop.
        var rooted = Set<String>()
        for s in list where parentOf[s.target.key] == nil {
            var stack = [s]
            while let at = stack.popLast() {
                if rooted.insert(at.target.key).inserted { stack.append(contentsOf: children[at.target.key] ?? []) }
            }
        }
        // In order: a real root, or the first card of a loop, starts a walk.
        for s in list where !seen.contains(s.target.key) && (parentOf[s.target.key] == nil || !rooted.contains(s.target.key)) { walk(s) }
        // Below the depth bound a card starts at top level again; nothing is hidden.
        for s in list where !seen.contains(s.target.key) { walk(s) }
        // Each parent's descendants, summed once from the leaves up.
        var teams: [String: SessionTeam] = [:]
        for node in nodes.reversed() {
            guard let parent = node.parent else { continue }
            let s = node.s
            let below = teams[s.target.key] ?? SessionTeam()
            var sum = teams[parent] ?? SessionTeam()
            let up = online(s)
            let kind: DesktopBadgeKind = up && !SessionFilters.archived(s) ? DesktopSessionPalette.nativeKind(s, up) : .neutral
            let isQuiet = Self.quiet(s.raw)
            // A quiet request waits for the parent; any other one needs the person (spec 2.3).
            let asks = kind == .attention && !s.raw.bool("review_later") && !s.raw.bool("mobile_review_later")
            let needsYou = !SessionNotificationPreferences.muted(s) && !isQuiet && (asks || SessionFilters.needsAttention(s, up))
            sum.total += 1 + below.total
            sum.working += (SessionFilters.working(s, up) ? 1 : 0) + below.working
            sum.asking += (asks && isQuiet ? 1 : 0) + below.asking
            sum.needsYou += (needsYou ? 1 : 0) + below.needsYou
            sum.errors += (kind == .error ? 1 : 0) + below.errors
            teams[parent] = sum
        }
        var hidden = Set<String>()
        return nodes.compactMap { node in
            // Hidden with a collapsed parent; never promoted to top level.
            if let parent = node.parent, hidden.contains(parent) { hidden.insert(node.s.target.key); return nil }
            let team = teams[node.s.target.key]
            if team != nil && collapsed(node.s) { hidden.insert(node.s.target.key) }
            guard let parent = node.parent else {
                let link = Self.parent(node.s.raw)
                return SessionCardEntry(session: node.s, team: team, fromParent: link, fromParentKey: link == nil ? nil : parentKey(node.s))
            }
            return SessionCardEntry(session: node.s, depth: node.depth, parentKey: parent, lastChild: node.last, team: team)
        }
    }

    /// A parent's durable ID: its computer's machine UUID and its session ID, or nil.
    public static func teamId(_ machine: String?, _ session: Session) -> String? {
        let id = session.raw.string("session_id")
        guard let machine, uuid(machine), uuid(id) else { return nil }
        return "\(machine)\n\(id)"
    }

    /// The stored collapsed parents, oldest first; malformed entries are dropped.
    public static func parseTeams(_ raw: [String]) -> [String] {
        var seen = Set<String>(), ids: [String] = []
        for id in raw {
            let parts = id.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
            guard parts.count == 2, uuid(parts[0]), uuid(parts[1]), seen.insert(id).inserted else { continue }
            ids.append(id)
        }
        return Array(ids.suffix(maxCollapsedTeams))
    }

    /// The list after one choice: the newest last, the oldest dropped first. Expanded is the
    /// default and is not stored.
    public static func withTeam(_ list: [String], _ id: String, collapsed: Bool) -> [String] {
        Array((list.filter { $0 != id } + (collapsed ? [id] : [])).suffix(maxCollapsedTeams))
    }

    /// The desktop entity palette (dark skin), indexed by a label's slot.
    static let palette: [UInt32] = [0x5FBFA5, 0x8EA9F4, 0xC098E6, 0xE7AE66, 0xE38F9E, 0x75BFCF, 0xACC577, 0x8D9BAA]

    /// A stable palette slot (0-7) for a label: the desktop's 31-hash over UTF-16 units.
    public static func labelSlot(_ label: String) -> Int {
        var hash: Int32 = 0
        for unit in label.utf16 { hash = hash &* 31 &+ Int32(unit) }
        return Int(abs(hash % 8))
    }

    /// The first grapheme of the label holding a Unicode letter or number (`[\p{L}\p{N}]`),
    /// uppercased; the provider's initial when there is none.
    public static func markLetter(_ label: String, provider: String = "") -> String {
        let letterOrNumber: Set<Unicode.GeneralCategory> = [.uppercaseLetter, .lowercaseLetter, .titlecaseLetter, .modifierLetter, .otherLetter,
                                                            .decimalNumber, .letterNumber, .otherNumber]
        let text = label.trimmingCharacters(in: .whitespacesAndNewlines)
        if let first = text.first(where: { $0.unicodeScalars.contains { letterOrNumber.contains($0.properties.generalCategory) } }) {
            return String(first).uppercased()
        }
        let initial = provider.prefix(1).uppercased()
        return initial.isEmpty ? "?" : initial
    }

    private static func luminance(_ color: UInt32) -> Double {
        func channel(_ shift: UInt32) -> Double {
            let c = Double((color >> shift) & 0xFF) / 255
            return c <= 0.03928 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    /// The ink with the stronger WCAG contrast on the color.
    public static func inkFor(_ color: UInt32) -> UInt32 {
        let l = luminance(color)
        func ratio(_ ink: UInt32) -> Double { let i = luminance(ink); return (max(i, l) + 0.05) / (min(i, l) + 0.05) }
        return ratio(0x10151B) >= ratio(0xFFFFFF) ? 0x10151B : 0xFFFFFF
    }

    /// The color the row's computer keeps for its account (`account_mark.color`, `#rrggbb`), if any.
    public static func hostColor(_ raw: JSONObject) -> UInt32? {
        guard case .string(let text)? = raw.object("account_mark")?["color"], text.utf8.count == 7, text.hasPrefix("#"),
              text.dropFirst().allSatisfy({ ("0"..."9").contains($0) || ("a"..."f").contains($0) }) else { return nil }
        return UInt32(text.dropFirst(), radix: 16)
    }

    /// The provider whose account the row recorded: its agent, with DeepSeek's profiles under dsh.
    static func accountProvider(_ session: Session) -> String {
        let explicit = session.raw.string("account_provider")
        if !explicit.isEmpty { return explicit }
        let cmd = session.raw.string("cmd").isEmpty ? session.agent : session.raw.string("cmd")
        return cmd == "deepseek" ? "dsh" : cmd
    }

    /// The row's account mark: its computer and recorded `account_id` against that computer's
    /// account catalog, labeled by the grouped account. A row without a recorded binding, or whose
    /// profile is not in the loaded catalog, shows no mark rather than a guess (spec 1.6).
    public static func accountMark(_ session: Session, accounts: [GroupedAccount]) -> AccountMark? {
        guard case .string(let id)? = session.raw["account_id"], !id.isEmpty else { return nil }
        let provider = accountProvider(session), machine = session.target.machineKey
        guard let group = accounts.first(where: { group in group.members.contains {
            $0.catalog.key == machine && $0.account.provider == provider && $0.account.id == id
        } }) else { return nil }
        let label = [group.label, group.snapshot.account.identity.email, provider].first { !$0.trimmingCharacters(in: .whitespaces).isEmpty } ?? provider
        let color = hostColor(session.raw) ?? palette[labelSlot(label)]
        return AccountMark(label: label, letter: markLetter(label, provider: provider), color: color, ink: inkFor(color), provider: provider)
    }
}
