import Foundation
import ZerusKit

/// Canonical history windows, saved positions and bounded head probes.
extension ConversationModel {
    func historyNetworkReason(_ target: Target) -> String {
        if target.run.isEmpty || target.conversation.isEmpty { return "History is still starting." }
        if !app.connections.contains(where: { $0.id == target.connectionId }) { return "No connection. Showing saved conversation." }
        guard let relay = app.relayOperations[target.connectionId], let native = app.machineOperations[target.machineKey] else {
            return "Connect to check earlier history availability."
        }
        if !relay.contains("history") || !native.contains("history") { return "This computer does not support earlier history yet." }
        return ""
    }

    func historyReason(_ target: Target) -> String {
        if target.run.isEmpty || target.conversation.isEmpty || !app.connections.contains(where: { $0.id == target.connectionId }) {
            return historyNetworkReason(target)
        }
        if readState.record(target)?.epoch.isEmpty == false || historyWindows[target.key] != nil { return "" }
        return historyNetworkReason(target)
    }

    /// One shared history read per exact request; at most four at a time. Full pages are retained for offline reading.
    func historyOnce(_ target: Target, _ direction: String, _ cursor: String, limit: Int = 100) async throws -> HistoryPage {
        let key = target.key + JSON.array([.string(direction), .string(cursor), .int(Int64(limit))]).compact
        if let flight = historyFlights[key] { return try await flight.task.value }
        while historyFlights.count >= 4 {
            if let first = historyFlights.values.first { _ = try? await first.task.value }
            try Task.checkCancellation()
            historyFlights = historyFlights.filter { !$0.value.task.isCancelled }
        }
        guard let connection = app.connections.first(where: { $0.id == target.connectionId }) else {
            throw ZerusError(.unavailable, "Workspace disconnected.")
        }
        let reason = historyNetworkReason(target)
        let client = app.client, pages = self.pages
        let flight = Task<HistoryPage, Error> {
            if !reason.isEmpty { throw ZerusError(.unavailable, reason) }
            let page = try await client.history(connection, target: target, direction: direction, cursor: cursor, limit: limit)
            self.projectionHeads[target.key] = (page.epoch, page.projectionApplied)
            while self.projectionHeads.count > 20, let oldest = self.projectionHeads.keys.filter({ $0 != target.key }).sorted().first { self.projectionHeads[oldest] = nil }
            self.scheduleProjection(target)
            if let pages, HistoryPageRetention.retains(limit: limit, page: page, direction: direction) {
                Task.detached(priority: .utility) { await pages.write(target, mode: direction, cursor: cursor, page: CachedHistoryPage(page)) }
            }
            return page
        }
        historyFlights[key] = (target, flight)
        defer { if historyFlights[key]?.task == flight { historyFlights[key] = nil } }
        return try await flight.value
    }

    func observeHistoryHead(_ page: HistoryPage, verified: Bool = true) {
        guard app.connections.contains(where: { $0.id == page.target.connectionId }) else { return }
        editRead { ConversationReadHead.observe($0, page) }
        let key = ConversationReadPolicies.key(page.target)
        if page.complete && verified { currentHistoryHeads.insert(key) } else { currentHistoryHeads.remove(key) }
        refreshReadAttention()
    }

    func publishWindow(_ target: Target, _ window: HistoryWindow, complete: Bool = false, indexing: Bool = false, boundary: String? = nil) {
        historyWindows[target.key] = window
        historyWindows.trim { $0.count > 20 }
        scheduleProjection(target)
        guard selected?.target == target else { return }
        if !readPositionReady { pendingHistoryPosition = target.key }
        pendingHistoryPublication = target.key
        guard let head = frames[target.key] ?? activeFrame.flatMap({ $0.target == target ? $0 : nil }) else { return }
        installFrame(head.with(events: window.events, truncated: head.truncated || window.hasBefore || window.hasAfter, window: window,
                               indexComplete: complete, indexing: indexing, unreadBoundary: boundary), verified: activityVerified)
    }

    @discardableResult
    func loadHistory(_ target: Target, _ direction: String, _ cursor: String = "", latest: Bool = false) -> Task<Void, Never>? {
        guard selected?.target == target, !olderLoading else { return nil }
        lastHistoryWasLatest = false
        if target.run.isEmpty || target.conversation.isEmpty || !app.connections.contains(where: { $0.id == target.connectionId }) {
            olderError = historyReason(target)
            return nil
        }
        let navigation = navigationId
        let intent = historyIntent
        lastHistoryRequest = (target, direction, cursor)
        olderLoading = true
        olderError = ""
        if ["", "around", "unread"].contains(direction) && !unreadBoundaryCaptured { readPositionReady = false }
        let task = Task { [weak self] in
            guard let self else { return }
            var published = false
            defer {
                if !published && self.selected?.target == target && self.navigationId == navigation && self.historyIntent == intent {
                    self.olderLoading = false
                    if self.pendingHistoryPosition != target.key { self.readPositionReady = true }
                }
            }
            do {
                let cursorEpoch: String?
                switch direction {
                case "before", "after": cursorEpoch = self.historyWindows[target.key]?.epoch
                case "around": cursorEpoch = self.readState.record(target)?.viewport?.epoch
                default: cursorEpoch = nil
                }
                let pages = self.pages
                let reopened = try await HistoryReadPaths.reopening(epoch: cursorEpoch, read: {
                    try await HistoryReadPaths.read(network: { try await self.historyOnce(target, direction, cursor) },
                                                    cached: { try await pages?.read(target, mode: direction, cursor: cursor) })
                }, head: { try await self.historyOnce(target, "", "") })
                try Task.checkCancellation()
                guard self.selected?.target == target, self.navigationId == navigation, self.historyIntent == intent else { return }
                let mode = reopened.head ? "" : direction
                let page = reopened.result.page, verified = reopened.result.verified
                if !verified { self.currentHistoryHeads.remove(ConversationReadPolicies.key(target)) }
                var known = Set(self.readState.record(target)?.seen ?? [])
                if let viewport = self.readState.record(target)?.viewport?.eventId { known.insert(viewport) }
                known.insert(self.firstUnreadEventId)
                let previous = self.activeFrame.flatMap { $0.target == target ? $0.events : nil } ?? []
                let knownIds = known
                let aliases = await Task.detached(priority: .userInitiated) {
                    RollingConversationHistory.merge(target: target, previousTarget: target, previous: previous, incoming: page.events).supersededIds
                        .merging(HistoryAliases.exact(page.events, known: knownIds)) { $1 }
                }.value
                try Task.checkCancellation()
                guard self.selected?.target == target, self.navigationId == navigation, self.historyIntent == intent else { return }
                if !aliases.isEmpty { self.editRead { $0.supersede(target, aliases) } }
                self.observeHistoryHead(page, verified: verified)
                guard self.selected?.target == target, self.navigationId == navigation, self.historyIntent == intent else { return }
                if mode.isEmpty && verified && !page.complete {
                    self.historyIndexing = page.indexing
                    self.historyComplete = false
                    self.olderLoading = false
                    self.readPositionReady = true
                    self.olderError = page.indexing ? "Earlier message history is still indexing. Try again shortly."
                        : "This computer could not provide earlier message history. Try again."
                    self.hasOlder = true
                    self.captureUnreadBoundary(target, self.events)
                    return
                }
                let marker = self.readState.record(target)
                let earliest = page.events.filter(ConversationReadPolicies.incoming).compactMap(\.incomingSeq).min()
                let saved = marker?.viewport
                let noSavedAnchor = saved == nil || saved?.epoch != page.epoch || saved?.cursor.isEmpty == true
                var unreadBeforePage = false
                if let latestIncoming = page.latestIncoming, let marker {
                    unreadBeforePage = latestIncoming > marker.readThrough && page.hasBefore && (earliest.map { $0 > marker.readThrough + 1 } ?? true)
                }
                if !latest && mode.isEmpty && verified && page.complete && marker?.established == true && noSavedAnchor && unreadBeforePage &&
                    !self.unreadBoundaryCaptured, let marker {
                    self.readPositionReady = false
                    let unread = try await self.historyOnce(target, "unread", HistoryRequests.unreadCursor(epoch: page.epoch, readThrough: marker.readThrough))
                    try Task.checkCancellation()
                    guard self.historyIntent == intent else { return }
                    self.observeHistoryHead(unread)
                    guard self.selected?.target == target, self.navigationId == navigation, self.historyIntent == intent else { return }
                    self.publishWindow(target, HistoryWindow.from(unread), complete: unread.complete, indexing: unread.indexing)
                    published = true
                    self.lastHistoryObservation = self.activeFrame?.observedAt ?? 0
                    return
                }
                let old = self.historyWindows[target.key]
                let window = await Task.detached(priority: .userInitiated) {
                    if (mode == "before" || mode == "after"), let old, old.epoch == page.epoch { return old.merge(page, direction: mode) }
                    return HistoryWindow.from(page)
                }.value
                try Task.checkCancellation()
                guard self.selected?.target == target, self.navigationId == navigation, self.historyIntent == intent else { return }
                self.publishWindow(target, window, complete: page.complete && verified, indexing: page.indexing, boundary: aliases[self.firstUnreadEventId])
                self.lastHistoryObservation = self.activeFrame?.observedAt ?? 0
                published = true
            } catch is CancellationError {
            } catch {
                if self.selected?.target == target && self.navigationId == navigation && self.historyIntent == intent {
                    self.olderError = ReadFailure.message(error, "conversation")
                    self.currentHistoryHeads.remove(ConversationReadPolicies.key(target))
                }
            }
        }
        olderTask = task
        return task
    }

    func retryHistory(_ target: Target) {
        if lastHistoryWasLatest { jumpToLatest(target); return }
        if let request = lastHistoryRequest, request.target == target { loadHistory(target, request.direction, request.cursor) }
        else { loadHistory(target, "") }
    }

    func loadOlder(_ target: Target) {
        guard let window = historyWindows[target.key] else {
            loadHistory(target, "")
            return
        }
        if window.hasBefore, let before = window.before { loadHistory(target, "before", before) }
    }

    func loadNewer(_ target: Target) {
        if let window = historyWindows[target.key], window.hasAfter, let after = window.after { loadHistory(target, "after", after) }
    }

    /// The native recent tail does not depend on historical indexing or exact unread totals.
    @discardableResult
    func jumpToLatest(_ target: Target) -> Task<Void, Never>? {
        guard selected?.target == target,
              let connection = app.connections.first(where: { $0.id == target.connectionId }) else { return nil }
        let navigation = navigationId
        lastHistoryWasLatest = true
        // Navigation may remain unchanged while an old asynchronous cache restore
        // passes through the same empty window. An explicit latest intent supersedes it.
        historyIntent += 1
        let intent = historyIntent
        latestHistoryIntent = intent
        olderTask?.cancel()
        viewportPageTask?.cancel()
        // Retire observers of the previous reading intent without cancelling the
        // shared native inspection, which the fresh recent read waits behind.
        detailGate.cancel()
        detailTask?.cancel()
        detailTimer?.cancel()
        detailBusy = false
        detailProgress = false
        presentationRevision += 1
        presentationTask?.cancel()
        olderLoading = true
        olderError = ""
        let task = Task { [weak self] in
            guard let self else { return }
            @MainActor func current() -> Bool {
                self.selected?.target == target && self.navigationId == navigation && self.historyIntent == intent && self.app.connections.contains(connection)
            }
            defer {
                if self.latestHistoryIntent == intent { self.latestHistoryIntent = nil }
                if current() { self.olderLoading = false; self.readPositionReady = true }
            }
            do {
                let inspected = try await self.inspectOnce(connection, target, fresh: true, current: current).value
                guard inspected.target == target else {
                    throw ZerusError(.unavailable, "The conversation changed. Refresh before loading its latest messages.")
                }
                guard current() else { return }
                guard !inspected.inspectedEvents.isEmpty || self.events.isEmpty else {
                    throw ZerusError(.unavailable, "This computer could not provide the latest messages. Loaded messages remain available; try again.")
                }
                let recent = await Task.detached(priority: .userInitiated) {
                    let bounded = RollingConversationHistory.merge(target: target, previousTarget: target, previous: [], incoming: inspected.inspectedEvents)
                    return inspected.with(events: bounded.events, truncated: inspected.truncated || bounded.truncated,
                                          window: nil, indexComplete: false, indexing: false)
                }.value
                guard current() else { return }
                self.historyWindows[target.key] = nil
                self.currentHistoryHeads.remove(ConversationReadPolicies.key(target))
                self.historyEpoch = ""
                self.historyComplete = false
                self.historyIndexing = false
                self.hasNewer = false
                if let last = recent.events.last {
                    self.editRead { $0.viewport(target, eventId: last.id, offset: 0, following: true) }
                }
                self.restoreHistoryAttempt = target.key + (self.readState.record(target)?.viewport?.cursor ?? "")
                self.pendingHistoryPublication = target.key
                self.rememberFrame(recent)
                self.installFrame(recent, verified: true)
                // Return only after the caller can choose its scroll target from
                // the newly installed rows, including an identical recent read.
                await self.presentationTask?.value
            } catch is CancellationError {
            } catch {
                if current() { self.olderError = ReadFailure.message(error, "conversation") }
            }
        }
        olderTask = task
        return task
    }

    func restoreSavedWindow(_ target: Target, navigation: Int64) {
        guard let saved = readState.record(target)?.viewport, !saved.following, !saved.cursor.isEmpty, let pages else { return }
        let intent = historyIntent
        let originalWindow = historyWindows[target.key]
        readPositionReady = false
        Task { [weak self] in
            var published = false
            defer {
                if let self, !published, self.selected?.target == target, self.navigationId == navigation,
                   self.historyIntent == intent, self.readState.record(target)?.viewport == saved,
                   !self.olderLoading, self.pendingHistoryPublication != target.key { self.readPositionReady = true }
            }
            guard let page = try? await HistoryReadPaths.saved(current: {
                self?.selected?.target == target && self?.navigationId == navigation && self?.historyIntent == intent &&
                    self?.readState.record(target)?.viewport == saved
            }, { try await pages.read(target, mode: "around", cursor: saved.cursor) }),
                  let self, self.selected?.target == target, self.navigationId == navigation, self.historyIntent == intent,
                  self.readState.record(target)?.viewport == saved, self.historyWindows[target.key] == originalWindow,
                  !self.events.contains(where: { $0.id == saved.eventId }) else { return }
            self.publishWindow(target, HistoryWindow.from(page))
            self.restoreHistoryAttempt = target.key + saved.cursor
            published = true
        }
    }

    func restoreHistory(_ target: Target) {
        guard historyNetworkReason(target).isEmpty, !olderLoading,
              let saved = readState.record(target)?.viewport, !saved.following, !saved.cursor.isEmpty else { return }
        let key = target.key + saved.cursor
        guard restoreHistoryAttempt != key else { return }
        restoreHistoryAttempt = key
        if !events.contains(where: { $0.id == saved.eventId }) {
            readPositionReady = false
            loadHistory(target, "around", saved.cursor)
        }
    }

    /// Bounded metadata-only probes keep unread counts current without opening every chat.
    func watchHistoryHeads(priority: Target? = nil) {
        guard (priority != nil || selected == nil), historyHeadTask == nil, readTrackingReady, !demo else { return }
        historyHeadTask = Task { [weak self] in
            guard let self else { return }
            defer { self.historyHeadTask = nil }
            let rows = self.app.sessions, state = self.readState, hosts = self.app.machines, workspaces = self.app.connections
            let relay = self.app.relayOperations, native = self.app.machineOperations
            let hints = self.historySnapshotHints, heads = self.currentHistoryHeads
            let candidates = await Task.detached(priority: .utility) { () -> [(Target, String)] in
                let connected = Set(workspaces.map(\.id))
                let online = Set(hosts.filter(\.online).map(\.key))
                let eligible = rows.filter { row in
                    let target = row.target
                    return !target.run.isEmpty && !target.conversation.isEmpty && connected.contains(target.connectionId) && online.contains(target.machineKey) &&
                        relay[target.connectionId]?.contains("history") == true && native[target.machineKey]?.contains("history") == true &&
                        (priority == nil || target == priority)
                }
                return ReadAllHeadCandidates.candidates(eligible, state).compactMap { row in
                    let hint = ReadAllHeadCandidates.hint(row)
                    let key = ConversationReadPolicies.key(row.target)
                    return hints[key] == hint && heads.contains(key) ? nil : (row.target, hint)
                }
            }.value
            for candidate in candidates { self.currentHistoryHeads.remove(ConversationReadPolicies.key(candidate.0)) }
            let chosen = self.historyHeadScheduler.choose(candidates.map(\.0), now: ProcessInfo.processInfo.systemUptime)
            guard !chosen.isEmpty else { return }
            for target in chosen {
                if priority == nil && self.selected != nil { return }
                var success = false
                do {
                    let page = try await self.historyOnce(target, "", "", limit: 1)
                    success = true
                    guard self.app.sessions.contains(where: { $0.target == target }), self.historyNetworkReason(target).isEmpty,
                          self.app.machines.contains(where: { $0.key == target.machineKey && $0.online }) else { continue }
                    self.observeHistoryHead(page, verified: priority != nil || self.selected?.target != target || self.historyWindows[target.key] != nil)
                    if let hint = candidates.first(where: { $0.0 == target })?.1 { self.historySnapshotHints[ConversationReadPolicies.key(target)] = hint }
                    self.adoptSharedReads()
                    if self.selected?.target == target { self.shareRead(target) }
                } catch {}
                self.historyHeadScheduler.result(target, now: ProcessInfo.processInfo.systemUptime, success: success)
            }
            // Continue only when this pass selected work; an empty catalog must settle.
            if priority == nil {
                Task { [weak self] in
                    try? await Task.sleep(for: .milliseconds(50))
                    self?.watchHistoryHeads()
                }
            }
        }
    }

    /// Debounced retention of the page around the reading position, so reopening restores it offline.
    func retainViewport(_ target: Target, cursor: String) {
        guard !cursor.isEmpty, let window = historyWindows[target.key], let pages else { return }
        viewportPageTask?.cancel()
        viewportPageTask = Task.detached(priority: .utility) {
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            if let retained = CachedHistoryPage.retained(window, anchor: cursor) { await pages.write(target, mode: "around", cursor: cursor, page: retained) }
            for neighbor in CachedHistoryPage.neighbors(window, anchor: cursor) {
                await pages.write(target, mode: neighbor.direction, cursor: neighbor.cursor, page: neighbor.page)
            }
        }
    }
}
