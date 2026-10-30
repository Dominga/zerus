// One temporary KWin script, scoped to our one normal workspace window.
(function () {
    let id = "", above = false, error = "unsupported";
    try {
        const matches = workspace.windowList().filter(function (w) {
            return w.pid === config.pid && w.normalWindow && !w.dialog && !w.deleted
                && String(w.resourceClass) === config.appId
                && (config.id ? String(w.internalId) === config.id : w.caption === config.caption);
        });
        error = matches.length === 0 ? "missing" : "ambiguous";
        if (matches.length === 1) {
            const w = matches[0];
            if (typeof w.keepAbove === "boolean" && w.internalId) {
                id = String(w.internalId);
                if (typeof config.desired === "boolean") w.keepAbove = config.desired;
                above = w.keepAbove;
                error = typeof config.desired === "boolean" && above !== config.desired ? "denied" : "";
            } else error = "unsupported";
        }
    } catch (_) { error = "unsupported"; }
    callDBus(config.service, config.path, "org.hgdev.Zerus.WindowLayer", "Report",
        config.token, id, above, error);
    callDBus(config.kwin, "/Scripting", "org.kde.kwin.Scripting", "unloadScript", config.plugin);
})();
